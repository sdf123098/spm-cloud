// Bundled locally with Three.js; no CDN or model-supplied network requests.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';

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
// Per-pack V orientation. Bedrock counts v downward while GL's UV origin is
// bottom-left, so the usual conversion is `1 - v/h`. A minority of packs ship
// UVs already in GL order; for those the flip samples the mirrored half of the
// sheet and the model comes out as loose fragments. `renderThumbnail` renders
// with the default and retries the other way when the frame is nearly empty.
let flipV = true;
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
  const names=['east','west','up','down','south','north'], [w,h,d]=dimensions;
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
  const pivot=new THREE.Group();const origin=vec(bedrock?(data.pivot||from.map((n,i)=>(n+to[i])/2)):(data.rotation?.origin||data.origin));pivot.position.set(...origin);mesh.position.sub(pivot.position);rotate(pivot,data.rotation?.angle!==undefined?vec([0,0,0]).map((n,i)=>['x','y','z'][i]===data.rotation.axis?data.rotation.angle:0):data.rotation);pivot.add(mesh);return pivot;
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
/**
 * An effect slab: a large, thin quad that belongs to a `ysmGlow*` bone.
 *
 * Judging by absolute size alone removes real geometry from packs modelled at a
 * large unit scale, so the test is relative to the body and also requires the
 * cube to be a sheet (one axis far thinner than the others). Blade flashes and
 * aura planes match; a glowing eye or core does not.
 */
function isEffectSlab(data, bodySpan) {
  const size = vec(data.size).map(Math.abs);
  const longest = Math.max(...size), shortest = Math.min(...size);
  if (!(bodySpan > 0)) return false;
  return longest > bodySpan * 0.25 && shortest < longest * 0.15;
}

function bedrock(value, images, declared, allCutout = false) {
  const geometry=value['minecraft:geometry']?.[0]||value;const bones=geometry.bones||[];
  const description=geometry.description||{};const resolution=[description.texture_width||geometry.texturewidth||64,description.texture_height||geometry.textureheight||64];
  // Extent of the whole model, so effect slabs can be judged against the body
  // rather than against a fixed unit count (packs disagree on unit scale).
  let bodySpan=0;
  {
    const lo=[Infinity,Infinity,Infinity],hi=[-Infinity,-Infinity,-Infinity];
    for(const b of bones) for(const c of b.cubes||[]){
      const o=vec(c.from||c.origin),s=vec(c.size);
      for(let i=0;i<3;i++){const a=Math.min(o[i],o[i]+s[i]),z=Math.max(o[i],o[i]+s[i]);if(a<lo[i])lo[i]=a;if(z>hi[i])hi[i]=z;}
    }
    if(Number.isFinite(lo[0])) bodySpan=Math.max(hi[0]-lo[0],hi[1]-lo[1],hi[2]-lo[2]);
  }
  // The pack's own declaration wins over any filename heuristic.
  const map=texture(declared?.data || primaryTexture(images));
  const root=new THREE.Group(), groups=new Map();let count=0;
  for(const bone of bones){const group=new THREE.Group();group.name=bone.name;group.position.set(...vec(bone.pivot));rotate(group,bone.rotation);groups.set(bone.name,{group,bone});}
  for(const {group,bone} of groups.values()){
    const parent=groups.get(bone.parent);let ancestor=parent;const visited=new Set([bone.name]);while(ancestor){if(visited.has(ancestor.bone.name))throw new Error('骨骼层级循环');visited.add(ancestor.bone.name);ancestor=groups.get(ancestor.bone.parent);}
    if(parent){group.position.sub(new THREE.Vector3(...vec(parent.bone.pivot)));parent.group.add(group);}else root.add(group);
    // YSM prefixes effect geometry with `ysmGlow` (blade flashes, auras, FX
    // sheets). The mod drives those from animation and composites them
    // additively, so in a static bind pose they are giant flat slabs parked over
    // the character: one pack has a 162x2x38 unit quad across a 176-unit body.
    // Rendered opaque they hide the model entirely and inflate the framing box
    // until the character is a speck. Drop the large ones; small glow pieces are
    // real detail (eyes, cores) and stay.
    const glow = /^ysmGlow/i.test(bone.name||'');
    for(const data of bone.cubes||[]){
      if(glow && isEffectSlab(data, bodySpan)) continue;
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

// Square, off-screen render of one payload, used by tools/thumbnail.html and the
// batch renderer. Deliberately shares buildModel/lighting/framing with mount()
// so the stored thumbnail matches what the admin card eventually shows; a
// separate code path would silently drift as the preview evolves.
/** Share of a 16x16 grid whose cells contain opaque pixels. */
function massScore(canvas) {
  const grid = 16, probe = document.createElement('canvas');
  probe.width = grid; probe.height = grid;
  const context = probe.getContext('2d');
  context.drawImage(canvas, 0, 0, grid, grid);
  const pixels = context.getImageData(0, 0, grid, grid).data;
  let filled = 0;
  for (let i = 3; i < pixels.length; i += 4) if (pixels[i] > 8) filled++;
  return filled / (grid * grid);
}

async function renderOnce(payload, size, flip) {
  flipV = flip;
  const card = await prepareCard(payload, size);
  try {
    const canvas = card.draw();
    return {url: canvas.toDataURL('image/png'), mass: massScore(canvas)};
  } finally { card.dispose(); }
}

/**
 * Render the card, trying both V orientations when the first comes out hollow.
 *
 * The flip is right for the vast majority of packs, but not all of them, and a
 * wrong guess is not subtle: the model samples the mirrored half of the atlas
 * and loses most of its surface. Measuring the finished frame is the only
 * reliable discriminator — alpha coverage of the atlas itself is not, because a
 * sparse effects atlas scores well while rendering nothing.
 */
export async function renderThumbnail(payload, size = 512) {
  const primary = await renderOnce(payload, size, true);
  if (primary.mass >= 0.25) return primary.url;
  const alternate = await renderOnce(payload, size, false);
  return alternate.mass > primary.mass ? alternate.url : primary.url;
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
    if (pose.animation && pose.bones === 0) {
      // Declared an animation but none of its bones matched the skeleton: the
      // pack is inconsistent, and a silently bind-posed still would look wrong.
      // Keep going (better than no thumbnail) but say so.
      console.warn(`preview_animation ${pose.animation} matched no bones`);
    }
    const {bounds} = isolateBody(model);
    if (bounds.isEmpty()) throw new Error('模型几何数据为空');
    const center = bounds.getCenter(new THREE.Vector3()), extent = bounds.getSize(new THREE.Vector3());
    model.position.sub(center);
    const scene = new THREE.Scene();
    scene.add(model, new THREE.AmbientLight(0xffffff, assets.noLighting?3:2));
    if(!assets.noLighting){const light = new THREE.DirectionalLight(0xffffff, 3);light.position.set(4, 8, 6);scene.add(light);}
    const camera = new THREE.PerspectiveCamera(35, width/figureHeight, 0.01, 10000);
    // Fit the frame, not a bounding sphere. `extent.length()/2` is the diagonal
    // radius, which for a tall narrow character (36×50×45) is dominated by the
    // largest axis and pushes the camera far enough that the body ends up at
    // ~35% of the frame. Instead solve the distance that makes the projected
    // height hit TARGET_FILL and the projected width stay inside the frame.
    const TARGET_FILL = 0.78;
    const halfFov = (camera.fov * Math.PI) / 360;
    // The view is off-axis, so the silhouette is not extent.x by extent.y; use
    // the horizontal span as seen from the camera's yaw.
    const rotates = previewRotates(payload);
    const yaw = rotates ? -4*Math.PI/180 : 0;
    const viewWidth = Math.abs(extent.x * Math.cos(yaw)) + Math.abs(extent.z * Math.sin(yaw));
    const viewHeight = extent.y;
    const byHeight = viewHeight / 2 / TARGET_FILL / Math.tan(halfFov);
    const byWidth = viewWidth / 2 / TARGET_FILL / Math.tan(halfFov) / camera.aspect;
    const distance = Math.max(byHeight, byWidth, 0.5);
    // Framing follows the pack, not a guess. ysm.json's
    // `properties.disable_preview_rotation` is the author's switch for the
    // client's turntable preview: when it is false the card rotates, and a
    // three-quarter view is the angle that reads best while turning. When it is
    // true the author wants a straight-on card, so the still is front-on.
    if (rotates) camera.position.set(Math.sin(yaw)*distance, 0, -Math.cos(yaw)*distance);
    // -Z is the character's facing direction (Minecraft/Bedrock convention), so
    // a non-rotating pack is viewed from straight behind that axis.
    else camera.position.set(0, 0, -distance);
    camera.lookAt(0, 0, 0);
    renderer ||= new THREE.WebGLRenderer({alpha: true, antialias: true, preserveDrawingBuffer: true});
    square = renderer;
    square.setPixelRatio(1);
    square.setSize(width, figureHeight, false);
    // Draw once, then read: a second render() before toDataURL() would publish a
    // half-presented buffer (blank in a headless swiftshader context).
    square.render(scene, camera);
    if (isBlank(square.domElement)) throw new Error('渲染结果为空（贴图未加载或几何不可见）');
    if (lacksMass(square.domElement)) throw new Error('渲染结果为无效几何（顶点尖刺，模型骨骼数据损坏）');
    const canvas=document.createElement('canvas');canvas.width=width;canvas.height=height;canvas.className='model-preview';
    const context=canvas.getContext('2d');
    const draw=(rotation=0)=>{
      model.rotation.y=rotation;square.setSize(width,figureHeight,false);square.render(scene,camera);
      context.clearRect(0,0,width,height);context.fillStyle='#323232';context.fillRect(0,0,width,height);
      context.imageSmoothingEnabled=false;
      if(background)context.drawImage(background,0,0,width,height);
      context.drawImage(square.domElement,0,0,width,figureHeight);
      if(foreground)context.drawImage(foreground,0,0,width,height);
      if(!art?.nameTop){context.fillStyle='#242424';context.fillRect(0,cover,width,height-cover);}
      context.fillStyle=art?.dark?'#161616':'#f0f0f0';context.font=`${Math.max(10,Math.round(height/90*7))}px sans-serif`;context.textBaseline='middle';
      let title=assets.name;while(title.length&&context.measureText(title).width>width-width/8)title=title.slice(0,-1);
      context.fillText(title,width/16,(cover+height)/2);
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
 * Reject a frame that is only whiskers.
 *
 * A degenerate triangle — one vertex flung to infinity by a bad bone transform —
 * paints a couple of hairlines across the whole canvas. `isBlank` passes, the
 * bounding box spans the frame, and the result looks like a full-size model in
 * any coarse check while being unusable. Require actual mass: sample a grid and
 * demand that a reasonable share of cells carry opaque pixels.
 */
function lacksMass(canvas, minimum = 0.02) {
  const probe = document.createElement('canvas');
  const grid = 16;
  probe.width = grid;
  probe.height = grid;
  const context = probe.getContext('2d');
  context.drawImage(canvas, 0, 0, grid, grid);
  const pixels = context.getImageData(0, 0, grid, grid).data;
  let filled = 0;
  for (let i = 3; i < pixels.length; i += 4) if (pixels[i] > 8) filled++;
  return filled / (grid * grid) < minimum;
}
/**
 * Hide meshes parked far outside the body, and return {bounds, hidden}.
 *
 * Two distinct causes produce the same symptom: a genuine helper cube placed
 * far away, and a broken export whose cube origins were written in the wrong
 * space (one pack has 47 cubes at y=-8450 next to a skeleton at y=25). Either
 * way a single stray mesh inflates Box3.setFromObject() to thousands of units
 * and the camera backs off until the character is a sub-pixel dot. Measuring
 * per-mesh and dropping the far tail keeps the model that IS drawable.
 */
function isolateBody(root) {
  const THREE_Box3 = THREE.Box3;
  const boxes = [];
  root.updateMatrixWorld(true);
  root.traverse(item => {
    if (!item.isMesh || !item.geometry?.attributes?.position) return;
    item.geometry.computeBoundingBox();
    const box = item.geometry.boundingBox.clone().applyMatrix4(item.matrixWorld);
    if (!Number.isFinite(box.min.x) || !Number.isFinite(box.max.x)) return;
    boxes.push({mesh: item, box, center: box.getCenter(new THREE.Vector3())});
  });
  const bounds = new THREE_Box3();
  if (!boxes.length) return {bounds, hidden: 0};

  const median = axis => {
    const values = boxes.map(b => b.center.getComponent(axis)).sort((a, b) => a - b);
    return values[Math.floor(values.length / 2)];
  };
  const mid = [median(0), median(1), median(2)];
  const distances = boxes.map(b => Math.hypot(b.center.x - mid[0], b.center.y - mid[1], b.center.z - mid[2]));
  const sorted = [...distances].sort((a, b) => a - b);
  const spread = sorted[Math.floor(sorted.length / 2)] || 1;
  // Generous: covers a limb swung far out, excludes a cube thousands of units off.
  const limit = Math.max(spread * 8, 2);
  // Framing ignores the tail of the distribution. Props and rideable furniture sit
  // hundreds of units from the body (one pack has a merry-go-round at |x|=106 for
  // a 89-unit-tall character); unioning them into the bounds shrinks the character
  // to a speck, which the mass guard then rejects as a "vertex spike". Trimming the
  // farthest 3% keeps the body — and its limbs — and drops the outliers.
  const framingLimit = Math.max(sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.97))] || 2, 2);

  // `ysmGlow*` geometry is animation-driven effects: blade flashes, auras, big flat
  // sheets parked around the body. It renders, but it never defines the frame.
  const inGlow = mesh => {
    for (let node = mesh; node; node = node.parent) {
      if (/^ysmGlow/i.test(node.name || '')) return true;
    }
    return false;
  };

  let hidden = 0;
  boxes.forEach((entry, index) => {
    if (distances[index] > limit) { entry.mesh.visible = false; hidden++; return; }
    if (inGlow(entry.mesh)) return;
    if (distances[index] > framingLimit) return;
    bounds.union(entry.box);
  });
  // A model whose every mesh is glow would frame nothing; fall back to all of it.
  if (bounds.isEmpty()) boxes.forEach(entry => bounds.union(entry.box));
  return {bounds, hidden};
}
/**
 * Apply the pack's own card pose.
 *
 * ysm.json declares `properties.preview_animation` — the animation YSM itself
 * plays while showing the model in a GUI card (`gui` in most packs). That is the
 * pose the client draws, so it is the pose the still must use; inventing framing
 * or falling back to `idle` when the pack said otherwise is what made thumbnails
 * disagree with the card. `idle` is only the fallback for packs that declare no
 * preview animation at all.
 *
 * Channels may be a constant, an array, or `{time: value}` keyframes; the LAST
 * keyframe is taken, i.e. the settled end of the loop.
 */
export function applyPreviewPose(root, payload) {
  const lastValue = value => {
    if (value == null) return null;
    if (typeof value === 'number' || Array.isArray(value)) return value;
    const keys = Object.keys(value).sort((a, b) => Number(a) - Number(b));
    return keys.length ? value[keys[keys.length - 1]] : null;
  };
  const wanted = previewAnimationName(payload);
  const files = payload.animations || [];
  const pick = (name) => files.find(file => file?.value?.animations?.[name]);
  // The mod queues only the author's configured preview animation.
  const chosen = [wanted]
    .filter(Boolean)
    .map(name => ({name, file: pick(name)}))
    .find(entry => entry.file);
  if (!chosen) return {animation: null, bones: 0};

  const animation = chosen.file.value.animations[chosen.name];
  // YSM animations are Molang: `"1+math.sin(30+q.anim_time*180)*0.01"`. There is
  // no evaluator here, and treating an unevaluable component as 0 is destructive:
  // a Root scale of `[1, "1+math.sin(...)", 1]` became [1, 0, 1] and flattened the
  // whole model to nothing, while an all-Molang Root scale became [0,0,0] and
  // hid it. Skip the channel instead — the bind pose is correct, a zeroed one is
  // not.
  const scalar = value => {
    if (typeof value === 'number') return Number.isFinite(value) ? value : null;
    if (typeof value === 'string' && value.trim() !== '') {
      const n = Number(value.trim());
      return Number.isFinite(n) ? n : null;
    }
    return null;
  };
  const triple = value => {
    if (!Array.isArray(value) || value.length < 3) return null;
    const out = [];
    for (let i = 0; i < 3; i++) {
      const n = scalar(value[i]);
      if (n === null) return null;
      out.push(n);
    }
    return out;
  };
  let applied = 0;
  root.traverse(node => {
    const track = animation?.bones?.[node.name];
    if (!track) return;
    const rotation = triple(lastValue(track.rotation));
    if (rotation) rotate(node, rotation);
    const position = triple(lastValue(track.position));
    if (position) node.position.add(new THREE.Vector3(...position));
    const raw = lastValue(track.scale);
    const flat = scalar(raw);
    const scale = flat !== null ? [flat, flat, flat] : triple(raw);
    if (scale) {
      // An authored 0 is a "hide this part" idiom, but a zeroed root erases the
      // model. Clamp instead of hiding: a tiny part stays invisible in practice,
      // and framing measures the posed model, so a uniformly scaled-down root
      // still comes out at full size.
      node.scale.set(...scale.map(v => Math.max(Math.abs(v), 0.01)));
    }
    applied++;
  });
  return {animation: chosen.name, bones: applied};
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
          // Old square screenshots predate authored cards; regenerate those locally.
          if(Math.abs(image.width/image.height-52/90)<.02){image.className='model-preview';image.alt=`${item.model.name} 的静态卡片`;item.visual.prepend(image);item.visual.querySelector('.preview-status')?.remove();item.visual.dataset.preview='ready';return;}
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
