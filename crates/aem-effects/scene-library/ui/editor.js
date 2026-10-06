/* Packaged plugin UI. The App frontend implements the protocol transport only. */
"use strict";
let state, definition, token, sequence=0;
const pending=new Map();
const status=document.getElementById("status");
function request(message) {
  if (!window.MotionStudioHost) return Promise.reject(new Error("宿主未连接"));
  const id=String(++sequence);
  return new Promise((resolve,reject)=>{
    const timeout=setTimeout(()=>{pending.delete(id);reject(new Error("宿主请求超时"));},10000);
    pending.set(id,{resolve,reject,timeout});
    window.MotionStudioHost.postMessage(JSON.stringify({protocol:1,token,id,message}));
  });
}
window.motionStudioReply=function(reply) {
  if (reply.token!==token) return;
  const task=pending.get(reply.id);if(!task)return;
  pending.delete(reply.id);clearTimeout(task.timeout);
  if(reply.ok)task.resolve(reply.result);else task.reject(new Error(reply.error||"编辑失败"));
};
window.motionStudioConnect=function(connection) {
  token=connection.token;state=connection.state;definition=connection.definition;
  document.getElementById("title").textContent=definition.name;render();preview();
};
async function edit(message) {
  try {
    message.revision=state.revision;
    state=await request(message);status.textContent="已更新";render();preview();
  } catch(e) {status.textContent=e.message;}
}
function element(tag,text,parent) {
  const node=document.createElement(tag);if(text)node.textContent=text;if(parent)parent.append(node);return node;
}
function number(label,value,min,max,change,parent) {
  const row=element("label",label,parent);const input=element("input","",row);
  input.type="number";input.step="0.01";input.min=String(min);input.max=String(max);input.value=String(value);
  input.addEventListener("change",()=>{const v=Number(input.value);if(Number.isFinite(v))change(v);});
}
function render() {
  const params=document.getElementById("params");params.replaceChildren();
  for(const p of definition.params) {
    const value=state.values[p.id];if(!value)continue;
    if(p.kind==="enum"||p.kind==="bool") {
      const row=element("label",p.name,params),select=element("select","",row);
      const options=p.kind==="bool"?["关闭","启用"]:p.options;
      options.forEach((name,i)=>{const option=element("option",name,select);option.value=String(i);});select.value=String(value[0]);
      select.addEventListener("change",()=>edit({op:"set",param:p.id,value:[Number(select.value),0,0,0]}));
    } else {
      const dimensions={float:1,vec2:2,vec3:3,color:4}[p.kind]||0;
      for(let i=0;i<dimensions;i++) number(p.name+(dimensions>1?" "+["X / R","Y / G","Z / B","A"][i]:"")+" "+p.units,value[i],p.min,p.max,v=>{const next=value.slice();next[i]=v;edit({op:"set",param:p.id,value:next});},params);
    }
  }
  const scene=document.getElementById("scene");scene.replaceChildren();
  if(!state.scene)return;
  number("随机种子",state.seed,0,4294967295,v=>edit({op:"seed",seed:Math.floor(v)}),scene);
  if(definition.renderer!=="lens_flare")return;
  element("h2","光源与镜头元件",scene);
  const source=element("label","跟随图层",scene),select=element("select","",source);
  const manual=element("option","手动位置",select);manual.value="";
  for(const layer of state.layers){const option=element("option",layer.name,select);option.value=String(layer.id);}
  select.value=state.scene.source_layer==null?"":String(state.scene.source_layer);
  select.addEventListener("change",()=>{const settings=structuredClone(state.scene);settings.source_layer=select.value?Number(select.value):null;edit({op:"scene",settings});});
  const row=element("label","启用图层 Alpha 遮挡",scene),check=element("input","",row);check.type="checkbox";check.checked=state.scene.occlusion;
  check.addEventListener("change",()=>{const settings=structuredClone(state.scene);settings.occlusion=check.checked;edit({op:"scene",settings});});
  state.scene.elements.forEach((item,index)=>{
    const box=element("fieldset","",scene);element("legend","#"+item.id+" "+item.shape,box);
    const update=(key,value)=>{const settings=structuredClone(state.scene);settings.elements[index][key]=value;edit({op:"scene",settings});};
    const type=element("select","",box);for(const name of ["glow","halo","ghost","streak","star"]){const option=element("option",name,type);option.value=name;}type.value=item.shape;type.addEventListener("change",()=>update("shape",type.value));
    number("位置偏移",item.offset,-8,8,v=>update("offset",v),box);
    number("宽度",item.size[0],1,8192,v=>update("size",[v,item.size[1]]),box);
    number("高度",item.size[1],1,8192,v=>update("size",[item.size[0],v]),box);
    number("强度",item.intensity,0,32,v=>update("intensity",v),box);
    number("星芒数",item.rays,2,32,v=>update("rays",Math.floor(v)),box);
    number("色差",item.chromatic,0,.5,v=>update("chromatic",v),box);
    for(let c=0;c<4;c++)number(["红","绿","蓝","Alpha"][c],item.color[c],0,1,v=>{const color=item.color.slice();color[c]=v;update("color",color);},box);
    const actions=element("div","",box);actions.className="actions";
    const enable=element("button",item.enabled?"停用":"启用",actions);enable.onclick=()=>update("enabled",!item.enabled);
    const remove=element("button","删除",actions);remove.onclick=()=>{const settings=structuredClone(state.scene);settings.elements.splice(index,1);edit({op:"scene",settings});};
    const move=element("button","上移",actions);move.disabled=index===0;move.onclick=()=>{const settings=structuredClone(state.scene);[settings.elements[index-1],settings.elements[index]]=[settings.elements[index],settings.elements[index-1]];edit({op:"scene",settings});};
  });
  const add=element("button","添加镜头元件",scene);add.disabled=state.scene.elements.length>=64;
  add.onclick=()=>{const settings=structuredClone(state.scene);settings.elements.push({id:Math.max(0,...settings.elements.map(e=>e.id))+1,shape:"glow",enabled:true,offset:0,size:[150,150],color:[1,1,1,1],intensity:1,rays:8,chromatic:0});edit({op:"scene",settings});};
}
async function preview() {
  try {
    const [width,height]=state.dimensions;
    const scale=320/Math.max(width,height);
    const image=await request({op:"preview",width:Math.max(1,Math.round(width*scale)),height:Math.max(1,Math.round(height*scale))});
    const decoded=new Image();decoded.onload=()=>{const canvas=document.getElementById("preview");canvas.width=image.width;canvas.height=image.height;canvas.getContext("2d").drawImage(decoded,0,0);};decoded.src="data:image/png;base64,"+image.png;
  }catch(e){status.textContent=e.message;}
}
document.getElementById("refresh").onclick=preview;
