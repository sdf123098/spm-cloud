// Bundled locally with Three.js; no CDN or model-supplied network requests.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';

let renderer, observer, generation = 0;
const scenes = new Set();
const paints = new Set();
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
function texture(source) {
  if (!source || !/^data:image\/(png|jpeg|webp|gif);base64,/i.test(source)) return null;
  const map = textureLoader.load(source, () => { for (const paint of paints) paint(); }); map.colorSpace = THREE.SRGBColorSpace; map.magFilter = THREE.NearestFilter; map.minFilter = THREE.NearestFilter;
  textureSizes.set(map, dimensions(source));
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
function material(map) { return new THREE.MeshLambertMaterial({map, color:map?0xffffff:0x719de0, transparent:!!map, alphaTest:.05, side:THREE.DoubleSide}); }
function cube(data, maps, resolution, bedrock = false) {
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
      const image=map?.image,known=textureSizes.get(map),width=image?.width||known?.width||resolution[0]||64,height=image?.height||known?.height||resolution[1]||64;
      const points=[[u,v],[u+du,v],[u,v+dv],[u+du,v+dv]];
      const turns=((face?.rotation||0)/90)%4;
      for(let j=0;j<4;j++){const p=points[turns? [0,1,3,2][([0,1,3,2].indexOf(j)+turns)%4]:j];uv.setXY(index*4+j,p[0]/width,1-p[1]/height);}
    }
    const mat=material(map);if(!bedrock && data.faces && (!face || face.texture===null)){mat.visible=false;}return mat;
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
function bedrock(value, images, declared) {
  const geometry=value['minecraft:geometry']?.[0]||value;const bones=geometry.bones||[];
  const description=geometry.description||{};const resolution=[description.texture_width||geometry.texturewidth||64,description.texture_height||geometry.textureheight||64];
  // The pack's own declaration wins over any filename heuristic.
  const map=texture(declared?.data || primaryTexture(images));
  const root=new THREE.Group(), groups=new Map();let count=0;
  for(const bone of bones){const group=new THREE.Group();group.position.set(...vec(bone.pivot));rotate(group,bone.rotation);groups.set(bone.name,{group,bone});}
  for(const {group,bone} of groups.values()){
    const parent=groups.get(bone.parent);let ancestor=parent;const visited=new Set([bone.name]);while(ancestor){if(visited.has(ancestor.bone.name))throw new Error('骨骼层级循环');visited.add(ancestor.bone.name);ancestor=groups.get(ancestor.bone.parent);}
    if(parent){group.position.sub(new THREE.Vector3(...vec(parent.bone.pivot)));parent.group.add(group);}else root.add(group);
    for(const data of bone.cubes||[]){if(++count>50000)throw new Error('预览几何数据过大');const mesh=cube(data,[map],resolution,true);mesh.position.sub(new THREE.Vector3(...vec(bone.pivot)));group.add(mesh);}
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
  return document.value.elements?bbmodel(document.value,payload.images):bedrock(document.value,payload.images,declared);
}
function dispose(root){root.traverse(item=>{item.geometry?.dispose();for(const mat of (Array.isArray(item.material)?item.material:[item.material])){mat?.map?.dispose();mat?.dispose();}});}

// Square, off-screen render of one payload, used by tools/thumbnail.html and the
// batch renderer. Deliberately shares buildModel/lighting/framing with mount()
// so the stored thumbnail matches what the admin card eventually shows; a
// separate code path would silently drift as the preview evolves.
export async function renderThumbnail(payload, size = 512) {
  const model = await buildModel(payload);
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
    scene.add(model, new THREE.AmbientLight(0xffffff, 2));
    const light = new THREE.DirectionalLight(0xffffff, 3);light.position.set(4, 8, 6);scene.add(light);
    const camera = new THREE.PerspectiveCamera(35, 1, 0.01, 10000);
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
    const yaw = rotates ? -0.25 : 0;
    const viewWidth = Math.abs(extent.x * Math.cos(yaw)) + Math.abs(extent.z * Math.sin(yaw));
    const viewHeight = extent.y;
    const byHeight = viewHeight / 2 / TARGET_FILL / Math.tan(halfFov);
    const byWidth = viewWidth / 2 / TARGET_FILL / Math.tan(halfFov);
    const distance = Math.max(byHeight, byWidth, 0.5);
    // Framing follows the pack, not a guess. ysm.json's
    // `properties.disable_preview_rotation` is the author's switch for the
    // client's turntable preview: when it is false the card rotates, and a
    // three-quarter view is the angle that reads best while turning. When it is
    // true the author wants a straight-on card, so the still is front-on.
    if (rotates) camera.position.set(-distance * 0.25, distance * 0.12, -distance);
    // -Z is the character's facing direction (Minecraft/Bedrock convention), so
    // a non-rotating pack is viewed from straight behind that axis.
    else camera.position.set(0, distance * 0.06, -distance);
    camera.lookAt(0, 0, 0);
    const square = new THREE.WebGLRenderer({alpha: true, antialias: true, preserveDrawingBuffer: true});
    square.setPixelRatio(1);
    square.setSize(size, size, false);
    // Draw once, then read: a second render() before toDataURL() would publish a
    // half-presented buffer (blank in a headless swiftshader context).
    square.render(scene, camera);
    const dataUrl = square.domElement.toDataURL('image/png');
    if (isBlank(square.domElement)) throw new Error('渲染结果为空（贴图未加载或几何不可见）');
    if (lacksMass(square.domElement)) throw new Error('渲染结果为无效几何（顶点尖刺，模型骨骼数据损坏）');
    return dataUrl;
  } finally {
    dispose(model);
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

  let hidden = 0;
  boxes.forEach((entry, index) => {
    if (distances[index] > limit) { entry.mesh.visible = false; hidden++; }
    else bounds.union(entry.box);
  });
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
function applyPreviewPose(root, payload) {
  const lastValue = value => {
    if (value == null) return null;
    if (typeof value === 'number' || Array.isArray(value)) return value;
    const keys = Object.keys(value).sort((a, b) => Number(a) - Number(b));
    return keys.length ? value[keys[keys.length - 1]] : null;
  };
  const wanted = previewAnimationName(payload);
  const files = payload.animations || [];
  const pick = (name) => files.find(file => file?.value?.animations?.[name]);
  // Declared animation first, then idle, then whatever the pack ships.
  const chosen = [wanted, 'idle', ...files.flatMap(file => Object.keys(file?.value?.animations || {}))]
    .filter(Boolean)
    .map(name => ({name, file: pick(name)}))
    .find(entry => entry.file);
  if (!chosen) return {animation: null, bones: 0};

  const animation = chosen.file.value.animations[chosen.name];
  let applied = 0;
  root.traverse(node => {
    const track = animation?.bones?.[node.name];
    if (!track) return;
    const rotation = lastValue(track.rotation);
    if (rotation != null) rotate(node, rotation);
    const position = lastValue(track.position);
    if (Array.isArray(position) && position.length >= 3) node.position.add(new THREE.Vector3(...vec(position)));
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
export function clear(){generation++;observer?.disconnect();paints.clear();for(const scene of scenes)dispose(scene);scenes.clear();}
export function mount(items, request) {
  clear();const current=generation, queue=[];let active=0;
  async function run(){if(active>=2 || !queue.length)return;const item=queue.shift();active++;
    try{
      const payload=await request(`/admin/api/assets/${encodeURIComponent(item.model.asset_id)}/preview`);if(current!==generation)return;
      const model=await buildModel(payload);if(current!==generation){dispose(model);return;}
      scenes.add(model);const bounds=isolateBody(model).bounds;if(bounds.isEmpty())throw new Error('模型几何数据为空');
      const center=bounds.getCenter(new THREE.Vector3()),size=bounds.getSize(new THREE.Vector3());model.position.sub(center);
      const scene=new THREE.Scene();scene.add(model,new THREE.AmbientLight(0xffffff,2));const light=new THREE.DirectionalLight(0xffffff,3);light.position.set(4,8,6);scene.add(light);
      const width=Math.max(item.visual.clientWidth,160),height=150;
      // Minecraft/Bedrock and Blockbench characters face -Z. View the face by
      // default, while keeping the model's own transforms intact.
      const camera=new THREE.PerspectiveCamera(35,width/height,.01,10000);const radius=Math.max(size.length()/2,.1),distance=radius/Math.sin(camera.fov*Math.PI/360)/Math.min(1,camera.aspect)*1.12;camera.position.set(-distance*.25,distance*.12,-distance);camera.lookAt(0,0,0);
      renderer ||= new THREE.WebGLRenderer({alpha:true,antialias:true,preserveDrawingBuffer:true});renderer.setPixelRatio(Math.min(devicePixelRatio,2));renderer.setSize(width,height);
      const canvas=document.createElement('canvas');canvas.width=renderer.domElement.width;canvas.height=renderer.domElement.height;canvas.className='model-preview';canvas.setAttribute('aria-label',`${item.model.name} 的 3D 预览`);const context=canvas.getContext('2d');
      const draw=()=>{if(current!==generation)return;renderer.setSize(width,height);renderer.render(scene,camera);context.clearRect(0,0,canvas.width,canvas.height);context.drawImage(renderer.domElement,0,0);};
      paints.add(draw);item.visual.querySelector('.preview-status')?.remove();item.visual.prepend(canvas);draw();
      canvas.addEventListener('pointermove',e=>{const box=canvas.getBoundingClientRect();model.rotation.y=(e.clientX-box.left)/box.width*1.6-.8;draw();});canvas.addEventListener('pointerleave',()=>{model.rotation.y=0;draw();});
      canvas.addEventListener('dblclick',()=>{model.rotation.y=0;draw();});
      item.visual.dataset.preview='ready';
    }catch(error){if(current===generation){const status=item.visual.querySelector('.preview-status');if(status){status.textContent=error.message;status.title=error.message;}item.visual.dataset.preview='failed';}}
    finally{active--;if(current===generation){run();run();}}
  }
  observer=new IntersectionObserver(entries=>{for(const entry of entries)if(entry.isIntersecting){observer.unobserve(entry.target);const item=items.find(i=>i.visual===entry.target);queue.push(item);}run();run();},{rootMargin:'80px'});
  for(const item of items){const status=document.createElement('span');status.className='preview-status';status.textContent='正在加载 3D 预览…';item.visual.append(status);observer.observe(item.visual);}
}
