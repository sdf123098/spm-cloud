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
function texture(source) {
  if (!source || !/^data:image\/(png|jpeg|webp|gif);base64,/i.test(source)) return null;
  const map = textureLoader.load(source, () => { for (const paint of paints) paint(); }); map.colorSpace = THREE.SRGBColorSpace; map.magFilter = THREE.NearestFilter; map.minFilter = THREE.NearestFilter; return map;
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
    if(rect){let [u,v,du,dv]=rect;const width=resolution[0]||64,height=resolution[1]||64;
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
function bedrock(value, images) {
  const geometry=value['minecraft:geometry']?.[0]||value;const bones=geometry.bones||[];
  const description=geometry.description||{};const resolution=[description.texture_width||geometry.texturewidth||64,description.texture_height||geometry.textureheight||64];
  const map=texture(images.find(i=>/texture|skin|player/i.test(i.path))?.data || images[0]?.data);
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
  return document.value.elements?bbmodel(document.value,payload.images):bedrock(document.value,payload.images);
}
function dispose(root){root.traverse(item=>{item.geometry?.dispose();for(const mat of (Array.isArray(item.material)?item.material:[item.material])){mat?.map?.dispose();mat?.dispose();}});}
export function clear(){generation++;observer?.disconnect();paints.clear();for(const scene of scenes)dispose(scene);scenes.clear();}
export function mount(items, request) {
  clear();const current=generation, queue=[];let active=0;
  async function run(){if(active>=2 || !queue.length)return;const item=queue.shift();active++;
    try{
      const payload=await request(`/admin/api/assets/${encodeURIComponent(item.model.asset_id)}/preview`);if(current!==generation)return;
      const model=await buildModel(payload);if(current!==generation){dispose(model);return;}
      scenes.add(model);const bounds=new THREE.Box3().setFromObject(model);if(bounds.isEmpty())throw new Error('模型几何数据为空');
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
