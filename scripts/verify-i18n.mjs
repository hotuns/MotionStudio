import ts from 'typescript';
import {readFileSync,readdirSync,statSync} from 'node:fs';
import {dirname,resolve,relative} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const read=path=>readFileSync(resolve(root,path),'utf8');
const walk=path=>readdirSync(path).flatMap(name=>{
  const child=resolve(path,name);
  return statSync(child).isDirectory()?walk(child):[child];
});
const english=JSON.parse(read('src/locales/en.json')),merged={};
const slots=text=>[...new Set([...text.matchAll(/\{(\d+)\}/g)].map(match=>match[1]))].sort().join(',');
for(const name of readdirSync(resolve(root,'src/locales')).filter(name=>/^en-.*\.json$/.test(name)).sort()){
  for(const [source,target] of Object.entries(JSON.parse(read('src/locales/'+name)))){
    if(source in merged&&merged[source]!==target)throw Error('Conflicting translation: '+source);
    if(typeof target!=='string'||(!target.trim()&&source!=='行'))throw Error('Empty translation: '+source);
    if(slots(source)!==slots(target))throw Error('Translation drops or adds a parameter: '+source);
    merged[source]=target;
  }
}
if(Object.keys(merged).length!==Object.keys(english).length||Object.entries(merged).some(([source,target])=>english[source]!==target)){
  throw Error('en.json differs from its reviewed en-*.json catalogs');
}
const frontend=new Map(),backend=new Map();
const add=(map,source,file)=>{
  source=source.trim().replace(/\s+/g,' ');
  if(/[\u3400-\u9fff]/.test(source))map.set(source,relative(root,file));
};
function concat(node){
  if(ts.isParenthesizedExpression(node))return concat(node.expression);
  if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken)return [...concat(node.left),...concat(node.right)];
  return [ts.isStringLiteralLike(node)?node.text:null];
}
for(const file of walk(resolve(root,'src')).filter(file=>/\.tsx?$/.test(file)&&!/[\\/]i18n-runtime[\\/]|[\\/]i18n\.ts$/.test(file))){
  const ast=ts.createSourceFile(file,readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true,file.endsWith('tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS);
  const visit=node=>{
    if(ts.isStringLiteralLike(node)||ts.isJsxText(node))add(frontend,node.text,file);
    if(ts.isTemplateExpression(node))add(frontend,node.head.text+node.templateSpans.map((span,index)=>'{'+index+'}'+span.literal.text).join(''),file);
    if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken&&!(ts.isBinaryExpression(node.parent)&&node.parent.operatorToken.kind===ts.SyntaxKind.PlusToken)){
      let index=0;const parts=concat(node);
      if(parts.includes(null))add(frontend,parts.map(part=>part??'{'+index+++'}').join(''),file);
    }
    ts.forEachChild(node,visit);
  };visit(ast);
}
for(const directory of ['crates/engine/src','crates/host/src','crates/filters/src','src-tauri/src']){
  for(const file of walk(resolve(root,directory)).filter(file=>file.endsWith('.rs'))){
    const source=readFileSync(file,'utf8').split('#[cfg(test)]')[0];
    for(const match of source.matchAll(/r(#{0,8})"([\s\S]*?)"\1|"(?:[^"\\]|\\[\s\S])*"/g)){
      let text=match[2];
      if(text===undefined){try{text=JSON.parse(match[0]);}catch{text=match[0].slice(1,-1);}}
      let index=0;add(backend,text.replace(/\{[^{}]*\}/g,()=>'{'+index+++'}'),file);
    }
  }
}
const missing=[...frontend,...backend].filter(([source])=>!(source in english));
if(missing.length)throw Error('Missing translations:\n'+missing.map(([source,file])=>file+': '+source).join('\n'));
console.log(JSON.stringify({catalogMessages:Object.keys(english).length,frontendMessages:frontend.size,backendMessages:backend.size,missing:0,placeholderChecks:true}));
