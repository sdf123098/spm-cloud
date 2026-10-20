(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;else root.SPMOfflineIdentity=api;})(typeof globalThis!=='undefined'?globalThis:this,function(){
  'use strict';
  function create(request,changed=()=>{}){
    let generation=0,loading=0;
    const empty=()=>({accountId:null,enabled:false,authorizationStatus:'NONE',identities:[],pending:false});let state=empty();
    function clear(){generation++;loading++;state=empty();changed(state);}
    async function load(){
      const current=++loading,epoch=generation;const data=await request('/account/api/offline-identities');
      if(current!==loading||epoch!==generation)return;
      if(state.accountId!==data.account_id){generation++;state.pending=false;}
      state={...state,accountId:data.account_id,enabled:!!data.enabled,authorizationStatus:data.authorization_status||'NONE',identities:data.identities||[]};changed(state);
    }
    async function revoke(identity){
      if(!state.accountId||state.pending)throw new Error('请先登录，或等待当前操作完成。');
      const epoch=generation;state={...state,pending:true};changed(state);
      try{await request(`/account/api/offline-identities/${encodeURIComponent(identity.identity_id)}/key`,{method:'DELETE'});if(epoch===generation)await load();}
      finally{if(epoch===generation){state={...state,pending:false};changed(state);}}
    }
    return {get state(){return state;},clear,load,revoke};
  }
  return {create};
});
