package com.motionstudio.editor

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.floor

private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
private fun array(vararg v:Any)=JSONArray(v.toList())
private fun transform()=JSONObject().put("position",track(array(0,0))).put("anchor",track(array(0,0)))
    .put("scale",track(array(100,100))).put("rotation",track(0)).put("skew",track(0)).put("skew_axis",track(0)).put("opacity",track(100))
private fun repeater()=JSONObject().put("copies",track(3)).put("offset",track(0)).put("position",track(array(100,0)))
    .put("anchor",track(array(0,0))).put("scale",track(array(100,100))).put("rotation",track(0))
    .put("start_opacity",track(100)).put("end_opacity",track(100)).put("composite","below")
private val names=mapOf("group" to "组","geometry" to "形状","fill" to "填充","stroke" to "描边","trim" to "修剪路径","repeater" to "中继器")

@Composable internal fun VectorGroupPanel(vm:EditorViewModel,vector:JSONObject,onItemChanged:()->Unit={}) {
    val root=vector.getJSONObject("source").getJSONObject("group")
    var selected by remember(vm.root,vm.selected){mutableLongStateOf(
        vm.property.takeIf{it.startsWith("vector:group:")}?.split(':')?.getOrNull(2)?.toLongOrNull()
            ?.takeIf{findGroupItem(root,it)!=null}?:root.getLong("id"))}
    var add by remember{mutableStateOf(false)}
    val item=findGroupItem(root,selected)?:root
    fun parent(group:JSONObject,id:Long):JSONObject? {
        for(child in group.optJSONArray("items").objects()) {
            val value=if(child.optString("kind")=="group")child.getJSONObject("group")else child
            if(value.optLong("id")==id)return group
            if(value.has("items"))parent(value,id)?.let{return it}
        };return null
    }
    val parent=parent(root,item.getLong("id"))
    val kind=if(item.has("items"))"group"else item.getString("kind")
    val id=item.getLong("id");fun key(p:String)="vector:group:$id:$p"
    fun choose(node:JSONObject) {
        val changed=selected!=node.getLong("id")
        selected=node.getLong("id")
        if(changed)onItemChanged()
        val parameter=if(node.has("items"))"position"else when(node.optString("kind")) {
            "geometry"->"position";"fill","stroke"->"color";"trim"->"end";"repeater"->"copies";else->"position"
        }
        vm.selectVectorTrack("vector:group:$selected:$parameter")
    }
    fun change(block:(JSONObject,JSONObject)->Unit){val next=JSONObject(vector.toString());val r=next.getJSONObject("source").getJSONObject("group")
        val chosen=findGroupItem(r,id)?:return;block(r,chosen);vm.replaceVector(next)}
    Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
        if(parent!=null)TextButton(onClick={choose(parent)},modifier=Modifier.testTag("group-back")){Text("返回上级")}
        Text(item.optString("name",names[kind]?:"组"),Modifier.weight(1f),fontSize=16.sp,color=Ink)
        if(parent!=null)TextButton(onClick={
            val next=JSONObject(vector.toString());val p=findGroupItem(next.getJSONObject("source").getJSONObject("group"),parent.getLong("id"))?:return@TextButton
            p.put("items",JSONArray(p.getJSONArray("items").objects().filter{(if(it.optString("kind")=="group")it.getJSONObject("group")else it).getLong("id")!=id}))
            vm.replaceVector(next);choose(parent)
        },enabled=vm.editable(),modifier=Modifier.testTag("group-delete")){Text("删除")}
    }
    when(kind) {
        "group"->{
            GroupPosition(vm,key("position"),"组位置")
            GroupPosition(vm,key("anchor"),"锚点")
            for(axis in 0..1)VectorNumber(vm,key("scale"),"缩放 ${if(axis==0)"X"else"Y"}（%）",-10000.0,10000.0,axis=axis)
            VectorNumber(vm,key("rotation"),"旋转（°）",-360000.0,360000.0)
            VectorNumber(vm,key("opacity"),"不透明度（%）",0.0,100.0)
            VectorNumber(vm,key("skew"),"倾斜（°）",-89.0,89.0)
            VectorNumber(vm,key("skew_axis"),"倾斜轴（°）",-360000.0,360000.0)
            Text("内容按顺序求值，填充和描边可独立编辑。",color=Muted,fontSize=12.sp)
            item.getJSONArray("items").objects().forEachIndexed{index,entry->
                val child=if(entry.optString("kind")=="group")entry.getJSONObject("group")else entry
                Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                    TextButton(onClick={choose(child)},modifier=Modifier.weight(1f).testTag("group-item-${child.getLong("id")}")){
                        Text(child.optString("name",names[entry.optString("kind")]?:"内容"),color=Ink)}
                    TextButton(onClick={change{_,g->val items=g.getJSONArray("items").objects().toMutableList();val value=items.removeAt(index);items.add(index-1,value);g.put("items",JSONArray(items))}},
                        enabled=vm.editable()&&index>0,modifier=Modifier.testTag("group-up-${child.getLong("id")}")){Text("↑")}
                    TextButton(onClick={change{_,g->val items=g.getJSONArray("items").objects().toMutableList();val value=items.removeAt(index);items.add(index+1,value);g.put("items",JSONArray(items))}},
                        enabled=vm.editable()&&index<item.getJSONArray("items").length()-1,modifier=Modifier.testTag("group-down-${child.getLong("id")}")){Text("↓")}
                }
            }
            Box {TextButton(onClick={add=true},enabled=vm.editable(),modifier=Modifier.heightIn(min=48.dp).testTag("group-add")){Text("＋添加内容")}
                DropdownMenu(expanded=add,onDismissRequest={add=false}){
                    for(type in listOf("group","geometry","fill","stroke","trim","repeater"))DropdownMenuItem(text={Text(names[type]!!)},modifier=Modifier.testTag("group-add-$type"),onClick={
                        add=false;change{r,g->val n=nextGroupId(r);val entry=when(type){
                            "group"->JSONObject().put("kind","group").put("group",JSONObject().put("id",n).put("name","组 $n").put("transform",transform()).put("items",JSONArray()))
                            "geometry"->{val parameters=JSONObject();vm.shapeCatalogue().first{it.getString("id")=="rectangle"}.getJSONArray("parameters").objects().forEach{parameters.put(it.getString("id"),track(it.getDouble("default")))}
                                JSONObject().put("kind",type).put("id",n).put("name","矩形 $n").put("size",track(array(100,100))).put("position",track(array(0,0)))
                                    .put("vector",JSONObject().put("source",JSONObject().put("kind","shape").put("shape","rectangle").put("parameters",parameters)).put("fill",JSONObject.NULL).put("stroke",JSONObject.NULL).put("fill_rule","non_zero"))}
                            "fill"->JSONObject().put("kind",type).put("id",n).put("name","填充 $n").put("color",track(array(.33,.86,.78,1))).put("fill_rule","non_zero").put("composite","below")
                            "stroke"->JSONObject().put("kind",type).put("id",n).put("name","描边 $n").put("stroke",JSONObject().put("color",track(array(1,1,1,1))).put("width",track(3)).put("cap","round").put("join","round").put("miter_limit",4)).put("composite","below")
                            "trim"->JSONObject().put("kind",type).put("id",n).put("name","修剪路径 $n").put("trim",JSONObject().put("start",track(0)).put("end",track(100)).put("offset",track(0)).put("mode","simultaneously"))
                            else->JSONObject().put("kind",type).put("id",n).put("name","中继器 $n").put("repeater",repeater())
                        };g.getJSONArray("items").put(entry)}
                    })
                }
            }
        }
        "geometry"->{
            val v=item.getJSONObject("vector");val source=v.getJSONObject("source")
            GroupPosition(vm,key("position"),"形状位置")
            if(source.optString("kind")=="shape") {
                for(axis in 0..1)VectorNumber(vm,key("size"),if(axis==0)"宽度"else"高度",0.0,32768.0,axis=axis)
                var shapeMenu by remember{mutableStateOf(false)}
                Box {TextButton(onClick={shapeMenu=true},enabled=vm.editable()){Text("形状 · ${source.getString("shape")}")}
                    DropdownMenu(expanded=shapeMenu,onDismissRequest={shapeMenu=false}){vm.shapeCatalogue().forEach{shape->DropdownMenuItem(text={Text(shape.getString("name"))},onClick={
                        shapeMenu=false;change{_,g->val p=JSONObject();shape.getJSONArray("parameters").objects().forEach{p.put(it.getString("id"),track(it.getDouble("default")))}
                            g.getJSONObject("vector").put("source",JSONObject().put("kind","shape").put("shape",shape.getString("id")).put("parameters",p))}
                    })}}}
                vm.shapeCatalogue().firstOrNull{it.getString("id")==source.getString("shape")}?.optJSONArray("parameters").objects().forEach{p->
                    VectorNumber(vm,key("shape:${p.getString("id")}"),p.getString("id"),p.getDouble("min"),p.getDouble("max"),p.optBoolean("discrete"))}
            }
            if(source.optString("kind")=="paths")GroupPathControls(vm,"vector:group:$id",source){paths->change{_,g->g.getJSONObject("vector").getJSONObject("source").put("paths",paths)}}
            for((field,label)in listOf("fill" to "填充颜色","stroke_color" to "描边颜色"))GroupColor(vm,key(field),label)
            v.optJSONObject("stroke")?.let{stroke->
                VectorNumber(vm,key("stroke_width"),"描边宽度",0.0,4096.0)
                VectorStrokeOptions(vm,stroke,"vector:group:$id"){next->change{_,g->g.getJSONObject("vector").put("stroke",next)}}
            }
        }
        "fill"->{GroupColor(vm,key("color"),"填充颜色");VectorOptions("绘制顺序",item.getString("composite"),listOf("below" to "位于前项下方","above" to "位于前项上方"),vm.editable()){value->change{_,g->g.put("composite",value)}}
            VectorOptions("填充规则",item.getString("fill_rule"),listOf("non_zero" to "非零","even_odd" to "奇偶"),vm.editable()){value->change{_,g->g.put("fill_rule",value)}}}
        "stroke"->{GroupColor(vm,key("color"),"描边颜色");VectorNumber(vm,key("width"),"描边宽度",0.0,4096.0)
            VectorStrokeOptions(vm,item.getJSONObject("stroke"),"vector:group:$id"){next->change{_,g->g.put("stroke",next)}}
            VectorOptions("绘制顺序",item.getString("composite"),listOf("below" to "位于前项下方","above" to "位于前项上方"),vm.editable()){value->change{_,g->g.put("composite",value)}}}
        "trim"->{VectorNumber(vm,key("start"),"开始（%）",0.0,100.0);VectorNumber(vm,key("end"),"结束（%）",0.0,100.0);VectorNumber(vm,key("offset"),"偏移（°）",-360000.0,360000.0)
            VectorOptions("多条路径",item.getJSONObject("trim").getString("mode"),listOf("simultaneously" to "同时修剪","individually" to "依次修剪"),vm.editable()){value->change{_,g->g.getJSONObject("trim").put("mode",value)}}}
        "repeater"->{VectorNumber(vm,key("copies"),"副本数量",0.0,1024.0);VectorNumber(vm,key("offset"),"偏移",-1024.0,1024.0)
            GroupPosition(vm,key("position"),"每份位移");GroupPosition(vm,key("anchor"),"锚点")
            for(axis in 0..1)VectorNumber(vm,key("scale"),"缩放 ${if(axis==0)"X"else"Y"}（%）",-1000.0,1000.0,axis=axis)
            VectorNumber(vm,key("rotation"),"旋转（°）",-360000.0,360000.0);VectorNumber(vm,key("start_opacity"),"开始透明度（%）",0.0,100.0);VectorNumber(vm,key("end_opacity"),"结束透明度（%）",0.0,100.0)
            VectorOptions("副本顺序",item.getJSONObject("repeater").getString("composite"),listOf("below" to "后方","above" to "前方"),vm.editable()){value->change{_,g->g.getJSONObject("repeater").put("composite",value)}}}
    }
}
@Composable private fun GroupColor(vm:EditorViewModel,key:String,label:String) {
    val value=vm.vectorValue(vm.selected,key) as? JSONArray?:return
    val objectId=vm.selected
    ColorProperty(vm,label,"group-color-$key",value,vm.editable(),onSelect={vm.selectVectorTrack(key)}){rgba,at->vm.setPropertyValue(objectId,key,at,rgba,false)}
}
@Composable internal fun GroupPosition(vm:EditorViewModel,key:String,label:String) {
    val value=vm.vectorValue(vm.selected,key) as? JSONArray?:return
    var pad by remember(vm.selected,key){mutableStateOf(false)}
    var captured by remember(vm.selected,key){mutableStateOf<JSONArray?>(null)}
    var at by remember{mutableIntStateOf(0)}
    fun edit(next:JSONArray,save:Boolean){vm.selectVectorTrack(key);vm.edit(JSONObject().put("op","set_vector").put("object",vm.selected).put("property",key).put("frame",at).put("value",next),save)}
    Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
        TextButton(onClick={vm.selectVectorTrack(key)},modifier=Modifier.weight(1f)){Text(label,color=if(vm.property==key)Accent else Ink)}
        TextButton(onClick={pad=!pad},enabled=vm.editable(),modifier=Modifier.heightIn(min=48.dp).testTag("group-pad-$key")){Text(if(pad)"滑轮"else"触控板")}
    }
    if(!pad)for(axis in 0..1)VectorNumber(vm,key,"${if(axis==0)"X"else"Y"}",-32768.0,32768.0,axis=axis)
    if(pad)TransformTouchpad(Modifier.fillMaxWidth().height(132.dp).testTag("group-touchpad-$key"),listOf(0,1),vm.editable(),onBegin={
        vm.pause();vm.selectVectorTrack(key);captured=JSONArray(value.toString());at=floor(vm.frame).toInt();vm.beginGesture()
    },onDelta={dx,dy,_->captured?.let{v->v.put(0,(v.getDouble(0)+dx).coerceIn(-32768.0,32768.0));v.put(1,(v.getDouble(1)+dy).coerceIn(-32768.0,32768.0));edit(JSONArray(v.toString()),false)}},
        onFinish={commit->if(captured!=null){if(commit)vm.endGesture()else vm.cancelGesture();captured=null}})
}
