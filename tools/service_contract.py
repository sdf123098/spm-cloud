#!/usr/bin/env python3
"""Run the compiled self-hosted service against an isolated cross-platform HTTP contract.

Uses only the Python standard library. Temporary credentials/data are never printed.
No request goes to the official Cloud. The optional --model checks original user model bytes.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def run(binary, model=None, registration_policy="restricted"):
    with tempfile.TemporaryDirectory(prefix="spm-contract-") as temporary:
        data=Path(temporary)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1",0)); port=sock.getsockname()[1]
        secret=uuid.uuid4().hex
        env=os.environ.copy()
        for key in list(env):
            if key.startswith("SPM_CLOUD_"): del env[key]
        env.update(SPM_CLOUD_BIND=f"127.0.0.1:{port}",SPM_CLOUD_ORIGIN="https://cloud.example.test",
            SPM_CLOUD_INSTANCE_ID="contract-test",SPM_CLOUD_DATABASE=str(data/"cloud.db"),
            SPM_CLOUD_OBJECT_DIR=str(data/"objects"),SPM_CLOUD_ACCESS_TOKEN=secret)
        if registration_policy != "default":
            env["SPM_CLOUD_ALLOW_SELF_REGISTRATION"] = "true" if registration_policy == "open" else "false"
        with (data/"service.log").open("wb") as log:
            process=subprocess.Popen([str(binary.resolve())],cwd=data,env=env,stdout=log,stderr=subprocess.STDOUT,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name=="nt" else 0)
            checks=[]
            base=f"http://127.0.0.1:{port}"
            def request(method,path,body=None,token=secret,status=200,headers=None,raw=False):
                h=dict(headers or {})
                if token: h["Authorization"]="Bearer "+token
                if body is not None and not isinstance(body,bytes):
                    body=json.dumps(body).encode();h["Content-Type"]="application/json"
                req=urllib.request.Request(base+path,data=body,headers=h,method=method)
                try: response=urllib.request.urlopen(req,timeout=15)
                except urllib.error.HTTPError as error: response=error
                with response:
                    content=response.read();response_headers=response.headers;actual=response.status
                assert actual in (status if isinstance(status, tuple) else (status,)),f"{method} {path}: expected {status}, received {actual}"
                checks.append(f"{method} {path} -> {actual}")
                if raw: return content,response_headers
                return json.loads(content) if content else None
            try:
                deadline=time.monotonic()+15
                while True:
                    try: request("GET","/health",token=None);break
                    except (urllib.error.URLError,ConnectionError):
                        if time.monotonic()>deadline or process.poll() is not None: raise RuntimeError("compiled service did not start")
                        time.sleep(.1)
                instance=request("GET","/v1/instance",token=None)
                assert instance["websocket_origin"]=="wss://cloud.example.test/v1/realtime"
                assert {"player_motion_v1","game_identity_auth_v1"}.issubset(instance["capabilities"])
                public_registration = registration_policy != "restricted"
                assert instance["auth"] == {"password_login":True,"game_identity_login":True,"game_identity_link":True,"self_registration":public_registration}
                _,cors=request("OPTIONS","/v1/assets",token=None,status=204,raw=True)
                assert cors["Access-Control-Allow-Origin"]=="*"
                request("POST","/v1/accounts",{"account_id":"anonymous","password":"test-anonymous-password"},token=None,status=201 if public_registration else 401)
                duplicate=request("POST","/v1/accounts",{"account_id":"account_local","password":"cannot-claim-bootstrap"},status=409)
                assert duplicate["code"]=="ACCOUNT_EXISTS"
                sessions={}
                for account in ["owner","observer"]:
                    request("POST","/v1/accounts",{"account_id":account,"password":"contract-password-123"},status=201)
                    sessions[account]=request("POST","/v1/sessions",{"account_id":account,"password":"contract-password-123"},token=None)
                owner=sessions["owner"]["access_token"];observer=sessions["observer"]["access_token"]
                assert request("GET","/v1/identities",token=owner)==[]
                duplicate=request("POST","/v1/accounts",{"account_id":"owner","password":"other-test-password"},status=409)
                assert duplicate["code"]=="ACCOUNT_EXISTS"
                providers=request("GET","/v1/identity-providers",token=None)
                assert {p["provider_id"] for p in providers}=={"official","littleskin","elyby","drasl_unmojang"}
                request("POST","/v1/identity-providers",{"provider_id":"extra","display_name":"Extra","base_url":"https://example.com","session_path":"/session/hasJoined","enabled":True},token=owner,status=403)
                request("POST","/v1/identity-providers",{"provider_id":"extra","display_name":"Extra","base_url":"https://example.com","session_path":"/session/hasJoined","enabled":True})
                assert request("POST","/v1/identity-providers",{"provider_id":"official","display_name":"Replacement","base_url":"https://example.com","enabled":True},status=403)["code"]=="ACCESS_DENIED"
                profile=str(uuid.uuid4())
                challenge=request("POST","/v1/auth/login-challenges",{"provider_id":"official","username":"Player","profile_uuid":profile},token=None)
                assert challenge["profile_key_payload"].split("\n")[1:6]==["https://cloud.example.test","login","","official",profile]
                complete=f'/v1/auth/login-challenges/{challenge["challenge_id"]}/complete'
                request("POST",complete,{"profile_key":{}},token=None,status=400)
                request("POST",complete,{"challenge_id":challenge["challenge_id"],"profile_key":{}},token=None,status=403)
                replay=request("POST",complete,{"challenge_id":challenge["challenge_id"],"profile_key":{}},token=None,status=401)
                assert replay["code"]=="IDENTITY_CHALLENGE_REPLAYED"
                link=request("POST","/v1/auth/challenges",{"provider_id":"official","username":"Player","profile_uuid":profile},token=owner)
                assert link["profile_key_payload"].split("\n")[1:6]==["https://cloud.example.test","link","owner","official",profile]
                complete_link=f'/v1/auth/challenges/{link["challenge_id"]}/complete'
                request("POST",complete_link,{"challenge_id":link["challenge_id"],"profile_key":{}},token=observer,status=401)
                request("POST",complete_link,{"profile_key":{}},token=owner,status=400)
                request("POST",complete_link,{"challenge_id":link["challenge_id"],"profile_key":{}},token=owner,status=403)
                assert request("POST",complete_link,{"challenge_id":link["challenge_id"],"profile_key":{}},token=owner,status=401)["code"]=="IDENTITY_CHALLENGE_REPLAYED"
                request("POST","/v1/identities",{"identity":f"official:{profile}","display_name":"Unverified"},token=owner,status=400)
                content=model.read_bytes() if model else b"\x00original-model\xff\r\n"
                sha=hashlib.sha256(content).hexdigest()
                def upload(asset,visibility="PRIVATE"):
                    return request("POST","/v1/assets",content,token=owner,status=201,headers={
                        "Idempotency-Key":uuid.uuid4().hex,"X-Asset-Id":asset,
                        "X-Asset-Name":urllib.parse.quote("芙宁娜v3.14日语配音.ysm"),
                        "X-Asset-Metadata-Encoding":"utf-8-percent","X-Asset-Format":"ysm",
                        "X-Asset-Sha256":sha,"X-Asset-Visibility":visibility})
                first=upload("contract_a");assert first["visibility"]=="PRIVATE"
                content_path="/v1/assets/contract_a/revisions/1/content"
                downloaded,headers=request("GET",content_path,token=owner,raw=True)
                assert downloaded==content and headers["Cache-Control"]=="no-store"
                request("GET",content_path,token=observer,status=403)
                request("PUT","/v1/assets/contract_a/visibility",{"visibility":"PUBLIC"},token=owner)
                assert request("GET",content_path,token=observer,raw=True)[0]==content
                assert request("GET",content_path,token=observer,status=206,headers={"Range":"bytes=1-5"},raw=True)[0]==content[1:6]
                request("GET",content_path,token=observer,status=304,headers={"If-None-Match":f'"{sha}"'},raw=True)
                request("GET",content_path,token=observer,status=416,headers={"Range":"bytes=999999999999-"})
                assert upload("contract_a","PUBLIC")["revision"]==2
                upload("contract_b","PUBLIC")
                query=urllib.parse.urlencode({"scope":"public","q":"芙宁娜","limit":1})
                page=request("GET","/v1/assets?"+query,token=observer)
                assert page["entries"][0]["revision"]==2 and page["next_cursor"]=="contract_a"
                page=request("GET","/v1/assets?"+query+"&after=contract_a",token=observer)
                assert page["entries"][0]["asset_id"]=="contract_b" and not page["has_more"]
                assert request("GET","/v1/assets?scope=public",token=observer,status=400)["code"]=="SEARCH_REQUIRED"
                request("PUT","/v1/assets/contract_a/visibility",{"visibility":"PRIVATE"},token=observer,status=403)
                request("PUT","/v1/assets/contract_a/visibility",{"visibility":"PRIVATE"},token=owner)
                request("GET",content_path,token=observer,status=403)
                request("PUT","/v1/assets/contract_a/acl",{"account_id":"observer","permission":"render_read"},token=owner)
                assert len(request("GET","/v1/assets?scope=shared",token=observer)["entries"])==1
                request("GET",content_path,token=observer,raw=True)
                # Seed verified identities only in this isolated fixture database; production has no bypass.
                with closing(sqlite3.connect(data/"cloud.db")) as conn, conn:
                    conn.execute("INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('fixture_owner','owner','official',?,'Player',1)",(profile,))
                appearance={"identity_id":"fixture_owner","entity_uuid":profile,"expected_revision":0,"asset_id":"contract_a","asset_revision":2,"raw_sha256":sha,"texture_id":"贴图 A"}
                request("PUT","/v1/players/me/appearance",appearance,token=owner)
                queried=request("POST","/v1/players/appearances/query",{"entity_uuids":[profile]},token=observer)
                assert queried["entries"][0]["selection"] is None
                request("PUT","/v1/assets/contract_a/visibility",{"visibility":"PUBLIC"},token=owner)
                queried=request("POST","/v1/players/appearances/query",{"entity_uuids":[profile]},token=observer)
                assert queried["entries"][0]["selection"]["texture_id"]=="贴图 A"
                request("PUT","/v1/players/me/appearance",appearance,token=observer,status=403)
                request("POST","/v1/scopes",{"scope_id":"scope_contract","name":"Contract","world_epoch":"epoch_contract"},token=owner,status=201)
                request("GET","/v1/scopes/scope_contract/targets",token=observer,status=403)
                request("PUT","/v1/scopes/scope_contract/acl",{"account_id":"observer","role":"viewer"},token=owner)
                assert request("POST","/v1/identities",{"identity":f"offline:scope_contract:{profile}","display_name":"Pending"},token=observer,status=403)["code"]=="SCOPE_ACCESS_DENIED"
                request("POST","/v1/targets",{"scope_id":"scope_contract","target_id":"target_contract","kind":"DUMMY","display_name":"Dummy"},token=owner,status=201)
                request("PUT","/v1/targets/target_contract/acl",{"account_id":"observer","role":"viewer"},token=owner)
                request("GET","/v1/scopes/scope_contract/targets",token=observer)
                appearance={"request_id":"appearance-contract","expected_revision":0,"asset_id":"contract_a","asset_revision":2,"raw_sha256":sha,"texture_id":"贴图 B","scale":1.0,"disabled":False}
                request("PUT","/v1/targets/target_contract/appearance",appearance,token=owner)
                request("PUT","/v1/targets/target_contract/appearance",appearance,token=owner)
                assert request("GET","/v1/targets/target_contract/appearance",token=observer)["texture_id"]=="贴图 B"
                request("GET","/v1/scopes/scope_contract/events/recovery?after=0",token=observer)
                animation={"request_id":"animation-contract","expected_revision":0,"channel":"base","action":"PLAY","animation_key":"idle","lease_ttl_ms":1000}
                request("PUT","/v1/targets/target_contract/animation",animation,token=owner)
                request("GET","/v1/targets/target_contract/animation",token=observer)
                request("PUT","/v1/scopes/scope_contract/acl",{"account_id":"observer","role":"editor"},token=owner)
                offline=request("POST","/v1/identities",{"identity":f"offline:scope_contract:{profile}","display_name":"Pending"},token=observer,status=201)
                assert offline["verification_status"]=="PENDING_VERIFICATION"
                request("POST","/v1/targets",{"scope_id":"scope_contract","target_id":"editor_contract","kind":"DUMMY","display_name":"Editor Dummy"},token=observer,status=201)
                # Actual release rollback: failed replacement insertion cannot revoke the old session.
                with closing(sqlite3.connect(data/"cloud.db")) as conn, conn:
                    conn.execute("CREATE TRIGGER reject_refresh BEFORE INSERT ON sessions BEGIN SELECT RAISE(ABORT,'test refresh rollback'); END;")
                request("POST","/v1/sessions/refresh",{"refresh_token":sessions["observer"]["refresh_token"]},token=None,status=500)
                request("GET","/v1/assets?scope=mine",token=observer)
                with closing(sqlite3.connect(data/"cloud.db")) as conn, conn:
                    conn.execute("DROP TRIGGER reject_refresh")
                with ThreadPoolExecutor(max_workers=2) as executor:
                    results=list(executor.map(lambda _: request("POST","/v1/sessions/refresh",{"refresh_token":sessions["observer"]["refresh_token"]},token=None,status=(200,401)), range(2)))
                assert sum("access_token" in result for result in results)==1
                assert sum(result.get("code")=="REFRESH_REUSED" for result in results)==1
                refresh=next(result for result in results if "access_token" in result)
                request("GET","/v1/assets?scope=mine",token=observer,status=401)
                request("POST","/v1/sessions/refresh",{"refresh_token":sessions["observer"]["refresh_token"]},token=None,status=401)
                request("DELETE","/v1/sessions/current",token=refresh["access_token"],status=204)
                request("GET","/v1/assets?scope=mine",token=refresh["access_token"],status=401)
                return {"passed_requests":len(checks),"model_bytes":len(content),"model_sha256":sha,"registration_policy":registration_policy,"checks":checks}
            finally:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill();process.wait()


if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--model",type=Path);parser.add_argument("--output",type=Path)
    parser.add_argument("--registration-policy",choices=["default","open","restricted"],default="restricted")
    args=parser.parse_args();result=run(args.binary,args.model,args.registration_policy)
    if args.output: args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    print(f'Compiled service HTTP contract passed: {result["passed_requests"]} requests, model {result["model_bytes"]} bytes')
