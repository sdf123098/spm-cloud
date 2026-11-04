/*! @license @bridge-editor/molang 2.0.2
MIT License

Copyright (c) 2020 solvedDev

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
// Bundled locally with Three.js; no CDN or model-supplied network requests.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { Molang } from '@bridge-editor/molang';

let renderer, observer, generation = 0;
const scenes = new Set();
const paints = new Set();
const cards = new Set();
const imageUrls = new Set();
const manager = new THREE.LoadingManager();
manager.setURLModifier(url => {
  if (/^data:(image\/(png|jpeg|webp|gif)|application\/octet-stream|application\/gltf-buffer);base64,/i.test(url) || url.startsWith('blob:')) return url;
  throw new Error('模型引用了外部资源，请上传包含内嵌贴图和缓冲区的模型');
});
const textureLoader = new THREE.TextureLoader(manager);
// UV normalisation needs the atlas's real pixel size, which only exists after
// the image decodes. buildModel() therefore awaits these dimensions; a pack
// that declares 512 UVs but ships a 1024 skin otherwise samples the wrong
// quadrant and renders fully transparent.
const textureSizes = new Map();
// One GPU texture per distinct image, shared by every material that uses it.
//
// `TextureLoader.load` never caches: each call decodes the image again and
// allocates a fresh GPU texture. A detailed pack has ~5700 cubes x 6 faces =
// 34k materials, all bound to the same 1024x1024 skin, so the uncached path
// asked the driver for 34k textures of 4 MB each. The upload silently fails and
// the model renders as dark garbage. Sharing keeps it at one.
const textureCache = new Map();
// Bedrock texture coordinates count V downward.
const flipV = true;
function texture(source) {
  if (!source || !/^data:image\/(png|jpeg|webp|gif);base64,/i.test(source)) return null;
  const cached = textureCache.get(source);
  if (cached) return cached;
  const map = textureLoader.load(source, () => { for (const paint of paints) paint(); }); map.colorSpace = THREE.SRGBColorSpace; map.magFilter = THREE.NearestFilter; map.minFilter = THREE.NearestFilter;
  textureSizes.set(map, dimensions(source));
  textureCache.set(source, map);
  return map;
}
/** Decode a data URL's intrinsic size without touching the GPU. */
function dimensions(source) {
  const match = /^data:image\/[a-z+]+;base64,(.*)$/i.exec(source);
  if (!match) return null;
  const bytes = atob(match[1].slice(0, 32));
  const code = c => c.charCodeAt(0);
  // PNG: IHDR width/height are the first two big-endian u32 after the magic.
  if (code(bytes[0]) === 0x89 && bytes.slice(1, 4) === 'PNG') {
    return {
      width: ((code(bytes[16]) << 24) | (code(bytes[17]) << 16) | (code(bytes[18]) << 8) | code(bytes[19])) >>> 0,
      height: ((code(bytes[20]) << 24) | (code(bytes[21]) << 16) | (code(bytes[22]) << 8) | code(bytes[23])) >>> 0,
    };
  }
  return null;
}
function vec(v, fallback = [0,0,0]) { return Array.isArray(v) && v.length >= 3 ? v.slice(0,3).map(n => Number.isFinite(Number(n)) ? Number(n) : 0) : fallback; }
function rotate(object, value) { const [x,y,z] = vec(value); object.rotation.set(x*Math.PI/180,y*Math.PI/180,z*Math.PI/180,'ZYX'); }
// Geometry stays in the JSON's coordinates beneath a reflected X root. In this
// space, the mod's baked (-X,-Y,+Z) rotations become (-X,+Y,-Z).
function rotateBedrock(object,value){const [x,y,z]=vec(value);rotate(object,[-x,y,-z]);}
// Always alpha-cutout, never blended.
//
// YSM skins are binary-alpha: a typical atlas is ~89% fully transparent with
// essentially nothing in the 1..13 range. Rendering them as `transparent` with
// `depthWrite: false` makes every cube rely on draw order, and a model with
// thousands of overlapping cubes then tears into floating fragments. Cutout
// matches what the mod does: discard texels below the threshold and keep the
// depth buffer authoritative.
function material(map) {
  return new THREE.MeshLambertMaterial({
    map,
    color: map ? 0xffffff : 0x719de0,
    transparent: false,
    alphaTest: map ? .5 : 0,
    depthWrite: true,
    side: THREE.DoubleSide,
  });
}
function cube(data, maps, resolution, bedrock = false, allCutout = false) {
  const from=vec(data.from||data.origin), to=data.to?vec(data.to):from.map((n,i)=>n+vec(data.size)[i]);
  const inflate=Number(data.inflate)||0, dimensions=from.map((n,i)=>Math.max(.001,to[i]-n+inflate*2));
  const geometry=new THREE.BoxGeometry(...dimensions), uv=geometry.attributes.uv;
  const names=bedrock?['west','east','up','down','south','north']:['east','west','up','down','south','north'], [w,h,d]=dimensions;
  const net={west:[0,d,d,h],north:[d,d,w,h],east:[d+w,d,d,h],south:[2*d+w,d,w,h],up:[d,0,w,d],down:[d+w,0,w,d]};
  const materials=names.map((name,index)=>{
    const face=data.faces?.[name] || (bedrock && !Array.isArray(data.uv) ? data.uv?.[name] : null);
    const map=maps[face?.texture ?? 0] || null;
    let rect;
    if(face?.uv){rect=bedrock?[...face.uv,...(face.uv_size||[w,h])]:[face.uv[0],face.uv[1],face.uv[2]-face.uv[0],face.uv[3]-face.uv[1]];}
    else if(bedrock && Array.isArray(data.uv)){const n=net[name];rect=[n[0]+data.uv[0],n[1]+data.uv[1],n[2],n[3]];}
    if(rect){let [u,v,du,dv]=rect;
      // Normalise against the atlas we actually loaded, not the geometry's
      // declared size. Packs routinely ship UVs in a 512 grid alongside a
      // 1024-wide skin; dividing by the declaration then samples the top-left
      // quadrant, which is transparent for most characters.
      // Divide by the geometry's declared UV grid, not the atlas pixel size.
      //
      // The two differ whenever a pack ships a hi-res sheet for a coarse grid:
      // 512px atlas with a 256 grid, 2048 with 512, 1024 with 512. Normalising
      // by the atlas then maps every UV into the top-left quadrant, so the model
      // samples a quarter of the sheet and renders as loose fragments.
      const width=resolution[0]||64,height=resolution[1]||64;
      const points=[[u,v],[u+du,v],[u,v+dv],[u+du,v+dv]];
      const turns=((face?.rotation||0)/90)%4;
      for(let j=0;j<4;j++){const p=points[turns? [0,1,3,2][([0,1,3,2].indexOf(j)+turns)%4]:j];uv.setXY(index*4+j,p[0]/width,flipV?1-p[1]/height:p[1]/height);}
    }
    const mat=material(map, allCutout);if((!bedrock && data.faces && (!face || face.texture===null)) || (bedrock && data.uv && !Array.isArray(data.uv) && !face)){mat.visible=false;}return mat;
  });
  const mesh=new THREE.Mesh(geometry,materials);
  mesh.position.set(...from.map((n,i)=>(n+to[i])/2));
  const pivot=new THREE.Group();const origin=vec(bedrock?(data.pivot||from.map((n,i)=>(n+to[i])/2)):(data.rotation?.origin||data.origin));pivot.position.set(...origin);mesh.position.sub(pivot.position);(bedrock?rotateBedrock:rotate)(pivot,data.rotation?.angle!==undefined?vec([0,0,0]).map((n,i)=>['x','y','z'][i]===data.rotation.axis?data.rotation.angle:0):data.rotation);pivot.add(mesh);return pivot;
}
function bbmodel(value, images) {
  const group=new THREE.Group();const resolution=[value.resolution?.width||64,value.resolution?.height||64];
  const maps=(value.textures||[]).map(t=>texture(t.source || images.find(i=>i.path.endsWith(t.path||t.name))?.data));
  const elements=new Map();if((value.elements||[]).length>50000)throw new Error('预览几何数据过大');
  for(const element of value.elements||[]){
    if(element.visibility===false)continue;
    if(element.from && element.to)elements.set(element.uuid,cube(element,maps,resolution));
    else if(element.vertices && element.faces){
      const points=[];for(const face of Object.values(element.faces)){const vertices=face.vertices||[];for(let i=1;i+1<vertices.length;i++)for(const id of [vertices[0],vertices[i],vertices[i+1]])points.push(...vec(element.vertices[id]));}
      const geo=new THREE.BufferGeometry();geo.setAttribute('position',new THREE.Float32BufferAttribute(points,3));geo.computeVertexNormals();const mesh=new THREE.Mesh(geo,material(null));mesh.position.set(...vec(element.origin));rotate(mesh,element.rotation);elements.set(element.uuid,mesh);
    }
  }
  const used=new Set();
  function add(items,parent,pivot=[0,0,0],depth=0){if(depth>128)throw new Error('骨骼层级过深');for(const item of items||[]){if(typeof item==='string'){const mesh=elements.get(item);if(mesh&&!used.has(item)){mesh.position.sub(new THREE.Vector3(...pivot));parent.add(mesh);used.add(item);}}else{const bone=new THREE.Group();const origin=vec(item.origin);bone.position.set(...origin.map((n,i)=>n-pivot[i]));rotate(bone,item.rotation);parent.add(bone);add(item.children,bone,origin,depth+1);}}}
  add(value.outliner,group);for(const [id,mesh]of elements)if(!used.has(id))group.add(mesh);return group;
}
/**
 * The texture the pack itself declares, from ysm.json's
 * `files.<section>.texture[].uv`. This is the author's choice; falling back to
 * guessing by filename is what previously picked a 32 px `arrow.png` for a
 * model whose geometry needs a 512 px atlas.
 */
function declaredTexture(payload, section) {
  const pack = payload.pack || {};
  for (const document of Object.values(pack)) {
    const files = document?.files || {};
    const order = [section, 'player', 'armor', 'main'].filter(Boolean);
    for (const key of [...order, ...Object.keys(files)]) {
      for (const entry of files?.[key]?.texture || []) {
        const wanted = entry?.uv || entry?.path;
        if (!wanted) continue;
        const match = payload.images.find(image => image.path.endsWith(wanted) || image.path.endsWith(wanted.split('/').pop()));
        if (match) return {data: match.data, path: match.path};
      }
    }
  }
  return null;
}
// Pick the Bedrock skin, not merely the first image whose path looks textual.
//
// The old `/texture|skin|player/i` test matched a path *segment*: a pack with
// `model/textures/arrow.png` and `model/textures/texture.png` always selected
// whichever sorted first, and rendering a 512-UV body against a 32 px arrow
// atlas samples outside the image — a fully transparent model that still
// encodes as a valid PNG. Prefer a basename that *is* a texture name, then the
// atlas by name and demote obvious body parts.
function primaryTexture(images) {
  if (!images.length) return null;
  // Match the basename, not the full path: `model/textures/arrow.png` used to
  // satisfy /texture/ because the *directory* is called textures.
  const named = /^(texture|skin|player|main)\.(png|jpg|jpeg|webp)$/i;
  const scored = images.map((image, index) => {
    const base = (image.path.split('/').pop() || '').toLowerCase();
    let score = 0;
    if (named.test(base)) score += 100;
    if (base.startsWith('texture')) score += 40;
    if (/\.(png|jpg|jpeg|webp)$/i.test(base)) score += 5;
    // Body parts and UI art, never the skin atlas.
    if (/arrow|avatar|item|icon|effect|cape|elytra/i.test(base)) score -= 50;
    return {image, score, index};
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored[0].image.data;
}
function bedrock(value, images, declared, allCutout = false) {
  const geometry=value['minecraft:geometry']?.[0]||value;const bones=geometry.bones||[];
  const description=geometry.description||{};const resolution=[description.texture_width||geometry.texturewidth||64,description.texture_height||geometry.textureheight||64];
  // The pack's own declaration wins over any filename heuristic.
  const map=texture(declared?.data || primaryTexture(images));
  const root=new THREE.Group(), groups=new Map();root.scale.x=-1;let count=0;
  for(const bone of bones){const group=new THREE.Group();group.name=bone.name;group.position.set(...vec(bone.pivot));rotateBedrock(group,bone.rotation);group.userData.bedrockRotation=vec(bone.rotation);groups.set(bone.name,{group,bone});}
  for(const {group,bone} of groups.values()){
    const parent=groups.get(bone.parent);let ancestor=parent;const visited=new Set([bone.name]);while(ancestor){if(visited.has(ancestor.bone.name))throw new Error('骨骼层级循环');visited.add(ancestor.bone.name);ancestor=groups.get(ancestor.bone.parent);}
    if(parent){group.position.sub(new THREE.Vector3(...vec(parent.bone.pivot)));parent.group.add(group);}else root.add(group);
    for(const data of bone.cubes||[]){
      if(++count>50000)throw new Error('预览几何数据过大');
      const mesh=cube(data,[map],resolution,true,allCutout);mesh.position.sub(new THREE.Vector3(...vec(bone.pivot)));group.add(mesh);
    }
  }
  return root;
}
export async function buildModel(payload) {
  const candidates=payload.documents.filter(d=>d.glb || d.value?.asset || d.value?.elements || d.value?.['minecraft:geometry'] || d.value?.bones);
  const document=candidates.find(d=>/(^|\/)(main|player|model)(\.geo)?\.(json|bbmodel)$/i.test(d.path)) || candidates.sort((a,b)=>{
    const count=d=>(d.value?.elements?.length||0)+(d.value?.['minecraft:geometry']?.[0]?.bones||d.value?.bones||[]).reduce((n,b)=>n+(b.cubes?.length||0),0);
    return count(b)-count(a);
  })[0];
  if(!document)throw new Error('模型缺少可显示的几何数据');
  if(document.glb || document.value?.asset){
    const data=document.glb?Uint8Array.from(atob(document.glb),c=>c.charCodeAt(0)).buffer:JSON.stringify(document.value);
    return (await new GLTFLoader(manager).parseAsync(data,'')).scene;
  }
  const declared=declaredTexture(payload, 'player');
  // `properties.all_cutout` decides blend vs cutout for the whole pack.
  const pack = Object.values(payload.pack || {})[0] || {};
  const allCutout = pack.properties?.all_cutout === true;
  return document.value.elements
    ? bbmodel(document.value, payload.images)
    : bedrock(document.value, payload.images, declared, allCutout);
}
function dispose(root){
  root.traverse(item=>{
    item.geometry?.dispose();
    for(const mat of (Array.isArray(item.material)?item.material:[item.material])) mat?.dispose();
  });
  // Textures are shared across materials, so they are released once from the
  // cache rather than per material.
  for(const map of new Set(textureCache.values())) map.dispose();
  textureCache.clear();
  textureSizes.clear();
}

// The same cover renderer feeds the WebUI and stored Cloud thumbnails.
export async function renderThumbnail(payload, size = 512) {
  const card = await prepareCard(payload, size);
  try { return card.draw().toDataURL('image/png'); }
  finally { card.dispose(); }
}

// The authored GUI layers use one complete 52:90 canvas, exactly as in the mod.
export function cardAssets(payload) {
  const entries = Object.entries(payload.pack || {});
  const [path, pack] = entries.find(([name]) => /(^|\/)ysm\.json$/i.test(name)) || entries[0] || ['', {}];
  const base = path.slice(0, path.lastIndexOf('/') + 1);
  const normalize = value => {
    const parts=[];
    for (const part of value.replace(/\\/g,'/').split('/')) {
      if (part==='..') parts.pop(); else if (part && part!=='.') parts.push(part);
    }
    return parts.join('/');
  };
  const image = key => {
    const wanted=pack.properties?.[key];
    if (!wanted) return null;
    const candidates=[normalize(base+wanted),normalize(base+'background/'+key+'.png')];
    const found=candidates.map(path=>(payload.images||[]).find(image=>normalize(image.path)===path)).find(Boolean);
    if (!found) throw new Error(`卡片素材缺失：${key}`);
    return found.data;
  };
  return {background:image('gui_background'), foreground:image('gui_foreground'), name:pack.metadata?.name||payload.name||'', noLighting:pack.properties?.gui_no_lighting===true};
}

async function loadCardImage(source) {
  if (!source) return null;
  if (!/^data:image\/(png|jpeg|webp|gif);base64,/i.test(source)) throw new Error('卡片素材必须内嵌');
  const image=new Image();image.src=source;await image.decode();return image;
}

// Port of ModelCardArtwork's alpha analysis: retain the author's caption space.
function artworkLayout(image) {
  if (!image) return null;
  const probe=document.createElement('canvas'), scale=Math.min(1,512/Math.max(image.width,image.height));
  probe.width=Math.max(1,Math.round(image.width*scale));probe.height=Math.max(1,Math.round(image.height*scale));
  const ctx=probe.getContext('2d');ctx.drawImage(image,0,0,probe.width,probe.height);
  const w=probe.width,h=probe.height,data=ctx.getImageData(0,0,w,h).data,rows=new Array(h).fill(0),inner=new Array(h).fill(0);
  let bottom=0;
  for(let y=0;y<h;y++)for(let x=0;x<w;x++){const alpha=data[(y*w+x)*4+3];if(alpha>16)bottom=y+1;if(alpha>192){rows[y]++;if(x>=Math.floor(w/8)&&x<w-Math.floor(w/8))inner[y]++;}}
  if(!bottom)return null;
  let figure=bottom,opening=false,nameTop=0;
  for(let y=Math.floor(h/4);y<bottom;y++){if(rows[y]<w/2)opening=true;else if(opening&&rows[y]>=Math.ceil(w*.8)){figure=y;break;}}
  if(bottom===h&&figure>=h/2&&h-figure>=Math.max(8,Math.floor(h/8))&&rows.slice(figure).every(n=>n>=Math.ceil(w*.8))){const top=figure+Math.max(2,Math.floor(h/45));if(h-top>=Math.max(8,Math.floor(h/8)))nameTop=top;}
  if(!nameTop&&h-bottom<=Math.max(3,Math.floor(h/16))){
    const dense=Math.ceil((w-2*Math.floor(w/8))*.7);let end=bottom-1;
    while(end>=h/2&&inner[end]<dense&&bottom-end<=Math.max(2,Math.floor(h/16)))end--;
    let top=end,gap=0;
    for(let y=end;y>=Math.floor(h/2);y--){if(inner[y]>=dense){top=y;gap=0;}else if(++gap>Math.max(1,Math.floor(h/90)))break;}
    const open=inner.slice(Math.floor(h/4),top).filter(n=>n<(w-2*Math.floor(w/8))/2).length;
    if(end-top+1>=Math.max(8,Math.floor(h/8))&&open>=Math.max(2,Math.floor(h/8))){figure=top;nameTop=top+Math.max(2,Math.floor(h/45));}
  }
  const luminances=[];
  const linear=n=>n<=.04045?n/12.92:((n+.055)/1.055)**2.4;
  if(nameTop)for(let y=nameTop+Math.max(1,Math.floor((bottom-nameTop)/6));y<bottom-Math.max(1,Math.floor((bottom-nameTop)/6));y++)for(let x=Math.floor(w/8);x<w-Math.floor(w/8);x++){
    const i=(y*w+x)*4,a=data[i+3]/255;
    luminances.push([.2126,.7152,.0722].reduce((sum,weight,c)=>sum+weight*linear((data[i+c]*a+50*(1-a))/255),0));
  }
  luminances.sort((a,b)=>a-b);
  return {bottom:bottom/h,figure:figure/h,nameTop:nameTop/h,dark:(luminances[Math.floor(luminances.length/2)]||0)>=.18};
}

async function prepareCard(payload, size = 512) {
  size=Math.max(90,Math.min(2048,Math.round(Number(size)||512)));
  const assets=cardAssets(payload);
  const [background,foreground]=await Promise.all([loadCardImage(assets.background),loadCardImage(assets.foreground)]);
  const frame=artworkLayout(foreground),art=frame||artworkLayout(background);
  const width=Math.round(size*52/90),height=size;
  const cover=Math.min(height-18,art&&(art.nameTop||art.bottom<1&&art.bottom>=.5)?Math.round(height*(art.nameTop||art.bottom)):Math.round(height*74/90));
  const figureHeight=Math.min(cover,Math.round(height*(frame?.figure||1)));
  const model = await buildModel(payload);
  let square;
  try {
    // Textures load asynchronously. Materials are `transparent: !!map`, so a
    // frame drawn before the maps decode renders the whole model fully
    // transparent — a valid PNG of nothing, which is exactly what shipped as
    // 51 identical 7 KB thumbnails. Wait for the manager's queue to drain and
    // the images to decode before measuring or drawing.
    await texturesReady(model);
    // Pose before measuring: the card pose changes the silhouette, so bounds
    // taken from the bind pose would frame the wrong thing.
    const pose = applyPreviewPose(model, payload);
    const properties = Object.values(payload.pack || {}).find(document => document?.properties)?.properties;
    if(properties){
      // IGeoRenderer.renderEarly uses heightScale for X/Z and widthScale for Y;
      // CustomPlayerRenderer then applies the vanilla player scale of 0.9375.
      const horizontal=Number(properties.height_scale ?? .7)*.9375;
      const vertical=Number(properties.width_scale ?? .7)*.9375;
      model.scale.multiply(new THREE.Vector3(horizontal,vertical,horizontal));
    }
    if (pose.animation && pose.bones === 0) {
      // Declared an animation but none of its bones matched the skeleton: the
      // pack is inconsistent, and a silently bind-posed still would look wrong.
      // Keep going (better than no thumbnail) but say so.
      console.warn(`preview_animation ${pose.animation} matched no bones`);
    }
    if (new THREE.Box3().setFromObject(model).isEmpty()) throw new Error('模型几何数据为空');
    const scene = new THREE.Scene();
    scene.add(model, new THREE.AmbientLight(0xffffff, assets.noLighting?3:2));
    if(!assets.noLighting){const light = new THREE.DirectionalLight(0xffffff, 3);light.position.set(4, 8, 6);scene.add(light);}
    // Match the mod's GUI projection, origin and scale (ModelPickerLayout and
    // renderCardFigure). Author animation translations/scales are composition:
    // auto-centering and fitting the posed bounds would erase them completely.
    const referenceHeight = figureHeight / height * 90;
    const guiScale = Math.min(referenceHeight, Math.max(16, Math.min(64, Math.round(referenceHeight * .43))));
    const pixelsPerUnit = guiScale / 16 * height / 90;
    const viewHeight = figureHeight / pixelsPerUnit;
    const centerY = (figureHeight / 2 - 3 * height / 90) / pixelsPerUnit;
    const camera = new THREE.OrthographicCamera(-width/pixelsPerUnit/2,width/pixelsPerUnit/2,viewHeight/2,-viewHeight/2,.01,10000);
    const rotates = previewRotates(payload);
    const yaw = rotates ? -4*Math.PI/180 : 0;
    const pitch = rotates ? 10*Math.PI/180 : 0;
    camera.position.set(Math.sin(yaw)*Math.cos(pitch)*1000,centerY+Math.sin(pitch)*1000,-Math.cos(yaw)*Math.cos(pitch)*1000);
    camera.lookAt(0,centerY,0);
    renderer ||= new THREE.WebGLRenderer({alpha: true, antialias: true, preserveDrawingBuffer: true});
    square = renderer;
    square.setPixelRatio(1);
    square.setSize(width, figureHeight, false);
    // Draw once, then read: a second render() before toDataURL() would publish a
    // half-presented buffer (blank in a headless swiftshader context).
    square.render(scene, camera);
    if (isBlank(square.domElement)) throw new Error('渲染结果为空（贴图未加载或几何不可见）');
    // Only the cover is published. SPM's Cloud list and the WebUI both own the
    // name row; baking it into the PNG duplicates it and changes cover cropping.
    const canvas=document.createElement('canvas');canvas.width=width;canvas.height=cover;canvas.className='model-preview';
    const context=canvas.getContext('2d');
    const draw=(rotation=0)=>{
      model.rotation.y=rotation;square.setSize(width,figureHeight,false);square.render(scene,camera);
      context.clearRect(0,0,width,height);context.fillStyle='#323232';context.fillRect(0,0,width,height);
      context.imageSmoothingEnabled=false;
      if(background)context.drawImage(background,0,0,width,height);
      context.drawImage(square.domElement,0,0,width,figureHeight);
      if(foreground)context.drawImage(foreground,0,0,width,height);
      return canvas;
    };
    return {draw,dispose(){dispose(model);}};
  } catch(error) {
    dispose(model);throw error;
  }
}

/**
 * Resolve once every texture in `root` actually has a decoded image, or after
 * `timeout` ms so a broken image cannot hang a batch run forever.
 *
 * Neither `manager.onLoad` nor `image.decode()` is usable here: the manager's
 * queue is drained before the browser decodes the ImageBitmap (measured ~2 s
 * later on a 9-texture model), and a texture created without a source has no
 * `image.decode()` to await. Poll the observable fact instead — every map has a
 * non-zero-width image.
 */
async function texturesReady(root, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    let pending = 0;
    root.traverse(item => {
      for (const mat of (Array.isArray(item.material) ? item.material : [item.material])) {
        if (mat?.map && !(mat.map.image && mat.map.image.width > 0)) pending++;
      }
    });
    if (!pending) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error('贴图解码超时');
}

/** True when the canvas holds no visible pixel — a PNG of nothing. */
function isBlank(canvas) {
  const probe = document.createElement('canvas');
  probe.width = canvas.width; probe.height = canvas.height;
  probe.getContext('2d').drawImage(canvas, 0, 0);
  const pixels = probe.getContext('2d').getImageData(0, 0, probe.width, probe.height).data;
  for (let i = 3; i < pixels.length; i += 4) if (pixels[i] > 8) return false;
  return true;
}

/**
 * Apply the pack's own card pose.
 *
 * ysm.json declares `properties.preview_animation` — the animation YSM itself
 * plays while showing the model in a GUI card (`gui` in most packs). That is the
 * pose the client draws, so it is the pose the still must use; inventing framing
 * made thumbnails disagree with the card. Standard pre_parallel/parallel slots
 * initialize variables and hide props around this pose, just as SPM does.
 *
 * All channels are sampled at the same instant. Bedrock keyframes can wrap
 * their values in pre/post objects, including a discontinuity at one timestamp.
 */
export function applyPreviewPose(root, payload, time = 1) {
  const wanted = previewAnimationName(payload);
  const files = payload.animations || [];
  const pick = (name) => files.find(file => file?.value?.animations?.[name]);
  // Match PlayerAnimationController / ParallelProcessor's standard slots.
  const chosen = [...Array.from({length:8},(_,i)=>`pre_parallel${i}`),wanted,...Array.from({length:8},(_,i)=>`parallel${i}`)]
    .filter(Boolean)
    .map(name => ({name, file: pick(name)}))
    .filter(entry => entry.file);
  if (!chosen.length) return {animation: null, bones: 0};
  const snapshotTime=Math.max(0,time);
  const ysm={equipped_enchantment_level:()=>0,effect_level:()=>0,relative_block_name:()=>'minecraft:air',particle:()=>0,play_sound:()=>0,first_order:(_id,target)=>target,second_order:(_id,target)=>target};
  for(const [axis,index] of ['x','y','z'].map((axis,i)=>[axis,i])){
    ysm[`bone_rot_${axis}`]=name=>{const bone=root.getObjectByName(name);return bone?bone.rotation[['x','y','z'][index]]*180/Math.PI:0;};
    ysm[`bone_pivot_abs_${axis}`]=name=>{const bone=root.getObjectByName(name);return bone?bone.getWorldPosition(bone.position.clone()).getComponent(index):0;};
  }
  // Use the Molang interpreter, with degree-based math and the same animation
  // time for every channel. Skipping an expression drops the author's entire
  // offset/scale (e.g. Mingfeng's Root and Furina's scrolling background bones).
  const molang = new Molang({query:{life_time:snapshotTime,position:()=>0,position_delta:()=>0,max_durability:()=>0,remaining_durability:()=>0},ctrl:{hold:()=>0},ysm},{useOptimizer:false,convertUndefined:false,variableHandler:name=>name==='query.anim_time'?time:undefined});
  const expression=value=>value.replace(/ysm\.(bone_rot|bone_pivot_abs)\(([^)]*)\)\.([xyz])/g,'ysm.$1_$3($2)');
  const evaluate=value=>{
    try{
      const tree=molang.parse(expression(value));
      tree.walk(node=>{
        // Undefined variables act as zero in arithmetic, but stay undefined on
        // the left of ?? (Suise's authored v.player_size??1 must remain one).
        if(node.type==='NameExpression'&&!node.cardRawEval){
          node.cardRawEval=node.eval.bind(node);node.eval=()=>node.cardRawEval()??0;
        }
        if(node.type==='GenericOperatorExpression'&&node.operator==='??'){
          node.eval=()=>{const [left,right]=node.allExpressions;return (left.cardRawEval?left.cardRawEval():left.eval())??right.eval();};
        }
        // Bedrock boolean values are numbers, including nested negation. The
        // library otherwise rejects !(a || !b) when its operand is boolean.
        if(node.type==='PrefixExpression'){
          const negate=node.toString().startsWith('!');
          node.eval=()=>negate?Number(!Number(node.allExpressions[0].eval())):-Number(node.allExpressions[0].eval());
          node.type='CardPrefixExpression';
        }
      });
      return tree.eval();
    }catch(error){throw new Error(`卡片表达式 ${value.slice(0,120)}：${error.message}`);}
  };
  let applied = 0;
  root.traverse(node=>{node.userData.cardBindPosition ||= node.position.clone();});
  for(const entry of chosen){
    const animation=entry.file.value.animations[entry.name];
    const length = Number(animation.animation_length) || 0;
    time = length > 0 ? (animation.loop === true ? snapshotTime % length : Math.min(snapshotTime,length)) : snapshotTime;
    for(const [timestamp,scripts] of Object.entries(animation.timeline||{})){
      if(Number(timestamp)>time)continue;
      // JsonMolangUtils evaluates timeline array entries independently by default.
      // Author comment/section lines carry no animation state (some contain
      // unescaped quote marks around bone names, so they are not string literals).
      for(const script of Array.isArray(scripts)?scripts:[scripts]){
        if(typeof script!=='string'||/^\s*[\[\]]\s*;?\s*$/.test(script)||/^\s*\[?\s*'.*'\s*;?\s*$/.test(script))continue;
        evaluate(script);
      }
    }
    const scalar = value => {
      if (typeof value === 'number') return Number.isFinite(value) ? value : null;
      if (typeof value === 'string' && value.trim() !== '') {
        const result = evaluate(value),n=typeof result==='boolean'?Number(result):result;
        // AnimationProcessor keeps the last valid channel for NaN/Inf. A freshly
        // initialized, hidden accessory can divide by its not-yet-set size.
        return typeof n==='number'&&Number.isFinite(n)?n:null;
      }
      return null;
    };
    const triple = value => {
      const uniform = scalar(value);
      if (uniform !== null) return [uniform, uniform, uniform];
      if (!Array.isArray(value) || value.length < 3) return null;
      const out = [];
      for (let i = 0; i < 3; i++) {
        const n = scalar(value[i]);
        if (n === null) return null;
        out.push(n);
      }
      return out;
    };
    const vector = (value, side = 'post') => triple(value?.[side] ?? value?.post ?? value?.pre ?? value);
    const sample = channel => {
      if (channel == null) return null;
      if (typeof channel !== 'object' || Array.isArray(channel) || 'post' in channel || 'pre' in channel) return vector(channel);
      const keys = Object.keys(channel).filter(key => Number.isFinite(Number(key))).sort((a,b) => Number(a)-Number(b));
      if (!keys.length) return null;
      const next = keys.findIndex(key => Number(key) > time);
      if (next === 0) return vector(channel[keys[0]], 'pre');
      if (next < 0) return vector(channel[keys.at(-1)]);
      const left = channel[keys[next-1]], right = channel[keys[next]];
      const a = vector(left), b = vector(right, 'pre');
      if (!a || !b) return null;
      const t = (time-Number(keys[next-1]))/(Number(keys[next])-Number(keys[next-1]));
      if (left.lerp_mode !== 'catmullrom' && right.lerp_mode !== 'catmullrom') return a.map((v,i) => v+(b[i]-v)*t);
      const before = vector(channel[keys[Math.max(0,next-2)]]) || a;
      const after = vector(channel[keys[Math.min(keys.length-1,next+1)]], 'pre') || b;
      return a.map((v,i) => .5*((2*v)+(-before[i]+b[i])*t+(2*before[i]-5*v+4*b[i]-after[i])*t*t+(-before[i]+3*v-3*b[i]+after[i])*t*t*t));
    };
    root.traverse(node => {
      const track = animation?.bones?.[node.name];
      if (!track) return;
      const rotation = sample(track.rotation);
      if (rotation) {
        const base=node.userData.bedrockRotation;
        if(base){
          const previous=entry.name.startsWith('parallel')?[-node.rotation.x*180/Math.PI,node.rotation.y*180/Math.PI,-node.rotation.z*180/Math.PI]:base;
          rotateBedrock(node,rotation.map((v,i)=>v+previous[i]));
        }else rotate(node,rotation);
      }
      const position = sample(track.position);
      if (position) node.position.copy(node.userData.cardBindPosition).add(new THREE.Vector3(...position));
      const scale = sample(track.scale);
      if (scale) {
        node.scale.set(...scale);
      }
      applied++;
    });
  }
  return {animation: wanted, bones: applied};
}

/** `properties.preview_animation` from ysm.json, if the pack declares one. */
function previewAnimationName(payload) {
  for (const document of Object.values(payload.pack || {})) {
    const name = document?.properties?.preview_animation;
    if (typeof name === 'string' && name) return name;
  }
  return null;
}
/**
 * Whether the client's card turntable applies to this pack:
 * ysm.json `properties.disable_preview_rotation`, defaulting to rotating
 * (the YSM default is `false`, i.e. rotation enabled).
 */
function previewRotates(payload) {
  for (const document of Object.values(payload.pack || {})) {
    const flag = document?.properties?.disable_preview_rotation;
    if (typeof flag === 'boolean') return !flag;
  }
  return true;
}
export function clear(){generation++;observer?.disconnect();paints.clear();for(const scene of scenes)dispose(scene);scenes.clear();for(const card of cards)card.dispose();cards.clear();for(const url of imageUrls)URL.revokeObjectURL(url);imageUrls.clear();renderer?.dispose();renderer?.forceContextLoss();renderer=undefined;}
export function mount(items, request, {staticImages=true}={}) {
  clear();const current=generation, queue=[];let active=0;
  async function run(){if(active>=2 || !queue.length)return;const item=queue.shift();active++;
    try{
      if(staticImages && item.model.thumbnail_status==='ready'){
        try{
          const blob=await request(`/admin/api/assets/${encodeURIComponent(item.model.asset_id)}/revisions/${item.model.revision}/thumbnail`,{responseType:'blob'});
          if(current!==generation)return;
          const url=URL.createObjectURL(blob);imageUrls.add(url);const image=new Image();image.src=url;await image.decode();
          if(current!==generation)return;
          // Old complete cards contain a baked name row. Regenerate their cover.
          if(image.width/image.height>52/90+.02){image.className='model-preview';image.alt=`${item.model.name} 的静态卡片`;item.visual.prepend(image);item.visual.querySelector('.preview-status')?.remove();item.visual.dataset.preview='ready';return;}
        }catch(error){if(current!==generation)return;if(error.status===401)throw error;}
      }
      const payload=await request(`/admin/api/assets/${encodeURIComponent(item.model.asset_id)}/preview`);if(current!==generation)return;
      payload.name ||= item.model.name;
      const card=await prepareCard(payload,512);if(current!==generation){card.dispose();return;}
      const canvas=card.draw();canvas.setAttribute('aria-label',`${item.model.name} 的${staticImages?'静态卡片':'交互卡片'}`);
      item.visual.querySelector('.preview-status')?.remove();item.visual.prepend(canvas);
      if(staticImages)card.dispose();else{
        cards.add(card);
        canvas.addEventListener('pointermove',e=>{if(current!==generation)return;const box=canvas.getBoundingClientRect();card.draw((e.clientX-box.left)/box.width*1.6-.8);});
        canvas.addEventListener('pointerleave',()=>{if(current===generation)card.draw();});
      }
      item.visual.dataset.preview='ready';
    }catch(error){if(current===generation){const status=item.visual.querySelector('.preview-status');if(status){status.textContent=error.message;status.title=error.message;}item.visual.dataset.preview='failed';}}
    finally{active--;if(current===generation){run();run();}}
  }
  observer=new IntersectionObserver(entries=>{for(const entry of entries)if(entry.isIntersecting){observer.unobserve(entry.target);const item=items.find(i=>i.visual===entry.target);queue.push(item);}run();run();},{rootMargin:'80px'});
  for(const item of items){const status=document.createElement('span');status.className='preview-status';status.textContent=item.model.deleted?'已删除模型无预览':`正在加载${staticImages?'静态卡片':'交互卡片'}…`;item.visual.append(status);if(!item.model.deleted)observer.observe(item.visual);}
}
