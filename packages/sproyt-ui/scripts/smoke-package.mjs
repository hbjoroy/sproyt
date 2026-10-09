import {mkdtempSync,rmSync,readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createElement} from 'react';
import {renderToString as renderReact} from 'react-dom/server';
import {createSSRApp,h} from 'vue';
import {renderToString as renderVue} from 'vue/server-renderer';
const dir=mkdtempSync(resolve('.package-smoke-'));
try {
 const result=spawnSync('tar',['-xzf',resolve('../../frontend/vendor/sproyt-ui-0.1.1.tgz'),'-C',dir]);
 if(result.status!==0)throw Error('Package extraction failed');
 const base=resolve(dir,'package');
 const manifest=JSON.parse(readFileSync(resolve(base,'package.json'),'utf8'));
 const sourceManifest=JSON.parse(readFileSync('package.json','utf8'));
 if(manifest.version!==sourceManifest.version)throw Error('Archive version differs from source');
 for(const entry of Object.values(manifest.exports)){
  for(const path of typeof entry==='string'?[entry]:[entry.import,entry.types]){
   if(!readFileSync(resolve(base,path)).equals(readFileSync(resolve(path))))throw Error(`Archive differs from built source: ${path}`);
  }
 }
 const react=await import(pathToFileURL(resolve(base,'dist/react/index.js')));
 const vue=await import(pathToFileURL(resolve(base,'dist/vue/index.js')));
 const a=renderReact(createElement(react.Theme,{mode:'dark'},createElement(react.Button,null,'Package check')));
 const b=await renderVue(createSSRApp({render:()=>h(vue.Theme,{mode:'dark'},()=>h(vue.Button,null,()=> 'Package check'))}));
 if(!a.includes('Package check')||!b.includes('Package check'))throw Error('Missing rendered content');
 const reactAuthor=renderReact(createElement(react.Message,{author:'Anne',time:'12:00',authorContent:createElement('button',null,'Edit status')},'Hello'));
 const vueAuthor=await renderVue(createSSRApp({render:()=>h(vue.Message,{author:'Anne',time:'12:00'},{author:()=>h('button','Edit status'),default:()=> 'Hello'})}));
 if(!reactAuthor.includes('Edit status')||!vueAuthor.includes('Edit status'))throw Error('Packed author slots do not render');
 console.log('Packed React and Vue exports render successfully; stylesheet and declarations present.');
}finally{rmSync(dir,{recursive:true,force:true});}
