import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';

test('authored card layers, static switch, saved images and mobile layout', async () => {
  const browser=await chromium.launch({args:['--use-gl=swiftshader','--enable-unsafe-swiftshader']});
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  try {
    await page.route('http://spm.test/**',async route=>{
      const path=new URL(route.request().url()).pathname;
      if(path.startsWith('/admin/')&&!path.startsWith('/admin/api/')||path==='/admin'){
        let file=path==='/admin/'||path==='/admin'?'admin.html':path.split('/').pop();
        if(file==='preview.js')file='preview.bundle.js';
        if(file==='app.js')file='admin.js';
        if(file==='style.css')file='admin.css';
        const contentType=file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.svg')?'image/svg+xml':'text/html';
        try{return await route.fulfill({body:await readFile(new URL('../web/'+file,import.meta.url)),contentType});}catch{return route.fulfill({status:404});}
      }
      const body=path==='/account/api/providers'?{providers:[]}:{};
      return route.fulfill({json:body});
    });
    await page.goto('http://spm.test/admin/');
    await page.waitForFunction(()=>typeof SPMPreview!=='undefined');
    const result=await page.evaluate(async()=>{
      const layer=(front=false)=>{const canvas=document.createElement('canvas');canvas.width=52;canvas.height=90;const c=canvas.getContext('2d');c.fillStyle=front?'#00ff00':'#ff0000';if(front){c.fillRect(0,0,3,90);c.fillRect(0,72,52,18);}else c.fillRect(0,0,52,90);return canvas.toDataURL();};
      const payload={documents:[{path:'models/main.json',value:{'minecraft:geometry':[{bones:[{name:'body',cubes:[{origin:[-4,0,-4],size:[8,24,8]}]}]}]}}],pack:{'ysm.json':{metadata:{name:'Test card'},properties:{gui_background:'bg.png',gui_foreground:'fg.png'}}},images:[{path:'bg.png',data:layer()},{path:'fg.png',data:layer(true)}]};
      const png=await SPMPreview.renderThumbnail(payload,540),image=new Image();image.src=png;await image.decode();
      const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const ctx=c.getContext('2d');ctx.drawImage(image,0,0);
      const pixel=(x,y)=>Array.from(ctx.getImageData(x,y,1,1).data);
      window.testPayload=payload;window.testPng=png;
      return {width:c.width,height:c.height,frame:pixel(4,100),background:pixel(40,40),figure:pixel(156,200),caption:pixel(40,490)};
    });
    assert.equal(result.width,312);assert.equal(result.height,540);
    assert.deepEqual(result.frame,[0,255,0,255]);assert.deepEqual(result.background,[255,0,0,255]);assert.deepEqual(result.caption,[0,255,0,255]);assert.notDeepEqual(result.figure,[255,0,0,255]);
    await page.evaluate(()=>{
      models=[{asset_id:'test',revision:1,name:'Test card',format:'ysm',moderation:'CLEAN',visibility:'PUBLIC',owner_account_id:'owner',byte_length:123}];
      document.querySelector('#login').hidden=true;document.querySelector('#console').hidden=false;
      for(const section of document.querySelectorAll('main.content>section'))section.hidden=section.id!=='tab-models';
      window.previewCalls=0;request=async(path,options)=>{previewCalls++;if(options?.responseType==='blob')return (await fetch(testPng)).blob();return structuredClone(testPayload);};
      setModelView('cards');
    });
    await page.waitForSelector('.model-visual[data-preview=ready] canvas');
    await page.locator('#static-model-images').uncheck();
    await page.waitForSelector('canvas[aria-label="Test card 的交互卡片"]');
    await page.locator('#static-model-images').check();
    await page.waitForSelector('canvas[aria-label="Test card 的静态卡片"]');
    assert.equal(await page.evaluate(()=>localStorage.getItem('spm-cloud-static-images')),'true');
    await page.evaluate(()=>{models[0].thumbnail_status='ready';renderModels();});
    await page.waitForSelector('img.model-preview');
    await page.setViewportSize({width:375,height:812});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    const ratio=await page.locator('.model-visual').evaluate(el=>el.clientWidth/el.clientHeight);
    assert.ok(Math.abs(ratio-52/90)<.02);
    if(process.env.SPM_CARD_SCREENSHOT)await page.screenshot({path:process.env.SPM_CARD_SCREENSHOT,fullPage:true});
    assert.deepEqual(errors,[]);
  } finally {await browser.close();}
});

test('local YSM parser output renders its own authored card', {skip:!process.env.SPM_YSM_FIXTURE_DIR}, async()=>{
  const root=process.env.SPM_YSM_FIXTURE_DIR,payload={documents:[],images:[],pack:{},animations:[]};
  for(const file of await readdir(root,{recursive:true})){
    if(!/\.(json|png)$/i.test(file))continue;
    const bytes=await readFile(path.join(root,file)),name=file.replaceAll('\\','/');
    if(name.endsWith('.png'))payload.images.push({path:name,data:'data:image/png;base64,'+bytes.toString('base64')});
    else {const value=JSON.parse(bytes);if(value['minecraft:geometry']||value.elements||value.bones)payload.documents.push({path:name,value});else if(value.animations)payload.animations.push({path:name,value});else if(value.properties)payload.pack[name]=value;}
  }
  const browser=await chromium.launch({args:['--use-gl=swiftshader','--enable-unsafe-swiftshader']});
  try{
    const page=await browser.newPage();await page.addScriptTag({path:fileURLToPath(new URL('../web/preview.bundle.js',import.meta.url))});
    const png=await page.evaluate(payload=>SPMPreview.renderThumbnail(payload,720),payload);
    await page.setContent('<style>body{margin:0}img{display:block}</style><img>');
    await page.locator('img').evaluate(async(image,png)=>{image.src=png;await image.decode();},png);
    if(process.env.SPM_YSM_SCREENSHOT)await page.locator('img').screenshot({path:process.env.SPM_YSM_SCREENSHOT});
    assert.equal(await page.locator('img').evaluate(image=>image.naturalHeight),720);
  }finally{await browser.close();}
});
