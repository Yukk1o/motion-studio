package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
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

@Composable internal fun MaskPanel(vm:EditorViewModel,modifier:Modifier,onCurveMode:(Boolean)->Unit) {
    var curve by remember(vm.root,vm.selected){mutableStateOf(false)}
    val masks=vm.masks();val mask=vm.maskData()
    LaunchedEffect(curve){onCurveMode(curve)}
    DisposableEffect(Unit){onDispose{onCurveMode(false)}}
    Column(modifier.background(Panel).testTag("mask-panel")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp).padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
            if(curve)TextButton(onClick={curve=false}){Text("返回蒙版")}else Text("蒙版",Modifier.weight(1f),color=Ink,fontSize=15.sp)
            if(!curve){TextButton(onClick={vm.addMask()},enabled=vm.editable()&&masks.size<16,modifier=Modifier.testTag("mask-add-rectangle")){Text("矩形")};TextButton(onClick={vm.addMask(true)},enabled=vm.editable()&&masks.size<16,modifier=Modifier.testTag("mask-add-pen")){Text("钢笔")}}
            Tool(Icons.Default.Close,"关闭蒙版",action=vm::closeWorkspace)
        }
        if(curve)CurveEditor(vm,Modifier.weight(1f).fillMaxWidth())else {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal=12.dp)) {
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {masks.forEach{m->TextButton(onClick={vm.chooseMask(m.getLong("id"))},modifier=Modifier.testTag("mask-instance-${m.getLong("id")}")){Text(m.getString("name"),color=if(vm.maskId==m.getLong("id"))Accent else Muted)}}}
                if(mask==null)Text("添加矩形蒙版，或使用钢笔在预览中绘制。",color=Muted,fontSize=13.sp)
                else {
                    val id=mask.getLong("id")
                    fun options(key:String,value:Any){vm.maskAction(JSONObject().put("kind","options").put("mask",id).put(key,value))}
                    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                        Text("启用",Modifier.weight(1f));Switch(mask.optBoolean("enabled",true),{options("enabled",it)},enabled=vm.editable(),modifier=Modifier.testTag("mask-enabled"))
                        Text("反转");Switch(mask.optBoolean("inverted"),{options("inverted",it)},enabled=vm.editable(),modifier=Modifier.testTag("mask-inverted"))
                    }
                    var modes by remember{mutableStateOf(false)}
                    val names=mapOf("none" to "无","add" to "相加","subtract" to "相减","intersect" to "交集","lighten" to "变亮","darken" to "变暗","difference" to "差值")
                    Box {TextButton(onClick={modes=true},enabled=vm.editable(),modifier=Modifier.testTag("mask-mode")){Text("模式 · ${names[mask.optString("mode","add")]}")};DropdownMenu(modes,{modes=false}){names.forEach{(key,label)->DropdownMenuItem(text={Text(label)},onClick={modes=false;options("mode",key)},modifier=Modifier.testTag("mask-mode-$key"))}}}
                    MaskNumber(vm,"mask:$id:opacity","不透明度 %",0.0,100.0)
                    MaskNumber(vm,"mask:$id:feather","羽化 X px",0.0,32000.0,0)
                    MaskNumber(vm,"mask:$id:feather","羽化 Y px",0.0,32000.0,1)
                    MaskNumber(vm,"mask:$id:expansion","扩展 px",-32000.0,32000.0)
                    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                        Text("闭合路径",Modifier.weight(1f));Switch(mask.getJSONObject("path").getBoolean("closed"),{closed->val path=JSONObject(mask.getJSONObject("path").toString()).put("closed",closed);vm.maskAction(JSONObject().put("kind","path").put("mask",id).put("path",path))},enabled=vm.editable()&&mask.getJSONObject("path").getJSONArray("nodes").length()>=3,modifier=Modifier.testTag("mask-closed"))
                    }
                    Row {TextButton(onClick={vm.vectorDrawMode=true},enabled=vm.editable(),modifier=Modifier.testTag("mask-draw")){Text("钢笔加点",color=if(vm.vectorDrawMode)Accent else Muted)};TextButton(onClick={vm.vectorDrawMode=false},modifier=Modifier.testTag("mask-edit-path")){Text("编辑节点",color=if(!vm.vectorDrawMode)Accent else Muted)}}
                    Text(if(vm.vectorDrawMode)"点按添加节点，拖动创建曲柄；闭合后参与蒙版。"else"在预览中选择并拖动节点或曲柄。",color=Muted,fontSize=12.sp)
                    if(vm.vectorNodeId!=0L) {
                        val key="mask:$id:node:${vm.vectorNodeId}"
                        listOf("节点 X","节点 Y","入柄 X","入柄 Y","出柄 X","出柄 Y").forEachIndexed{i,label->MaskNumber(vm,key,label,-32768.0,32768.0,i)}
                        TextButton(onClick={val path=JSONObject(mask.getJSONObject("path").toString());val nodes=path.getJSONArray("nodes").objects().filter{it.getLong("id")!=vm.vectorNodeId};path.put("nodes",JSONArray(nodes));if(nodes.size<3)path.put("closed",false);vm.maskAction(JSONObject().put("kind","path").put("mask",id).put("path",path));vm.vectorNodeId=0},enabled=vm.editable(),modifier=Modifier.testTag("mask-delete-node")){Text("删除节点")}
                    }
                    Row(Modifier.horizontalScroll(rememberScrollState())) {
                        TextButton(onClick={val index=masks.indexOfFirst{it.getLong("id")==id};val order=masks.map{it.getLong("id")}.toMutableList();order.removeAt(index);order.add(index-1,id);vm.maskAction(JSONObject().put("kind","reorder").put("masks",JSONArray(order)))},enabled=vm.editable()&&masks.indexOfFirst{it.getLong("id")==id}>0,modifier=Modifier.testTag("mask-move-up")){Text("上移")}
                        TextButton(onClick={val copy=JSONObject(mask.toString()).put("id",(masks.maxOfOrNull{it.getLong("id")}?:0)+1).put("name",mask.getString("name")+" 副本");vm.maskAction(JSONObject().put("kind","add").put("mask",copy))},enabled=vm.editable()&&masks.size<16,modifier=Modifier.testTag("mask-duplicate")){Text("复制")}
                        TextButton(onClick={vm.maskAction(JSONObject().put("kind","remove").put("mask",id));vm.maskId=0},enabled=vm.editable(),modifier=Modifier.testTag("mask-delete")){Text("删除蒙版")}
                    }
                }
            }
            Row(Modifier.fillMaxWidth().height(48.dp),verticalAlignment=Alignment.CenterVertically) {
                Tool(Icons.Default.SkipPrevious,"上一关键帧",vm.keys().any{it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
                Box(Modifier.testTag("mask-key")){Tool(if(vm.currentKey()!=null)Icons.Default.Diamond else Icons.Default.Add,"蒙版关键帧",vm.editable()&&vm.maskTrackRaw(vm.selected,vm.property)!=null,vm::toggleKey)}
                Tool(Icons.Default.SkipNext,"下一关键帧",vm.keys().any{it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
                TextButton(onClick={curve=true},enabled=vm.editable()&&vm.easingSegment()!=null,modifier=Modifier.testTag("mask-curve")){Text("曲线")}
            }
        }
    }
}

@Composable private fun MaskNumber(vm:EditorViewModel,key:String,label:String,min:Double,max:Double,axis:Int?=null) {
    val sample=vm.maskValue(vm.selected,key)?:return
    val value=if(axis==null)(sample as? Number)?.toDouble()?:return else (sample as? JSONArray)?.optDouble(axis)?:return
    var draft by remember(vm.selected,key,axis){mutableStateOf<Double?>(null)}
    var captured by remember{mutableStateOf<Any?>(null)};var at by remember{mutableIntStateOf(0)}
    var input by remember{mutableStateOf(false)}
    fun set(next:Double,save:Boolean){vm.property=key;val v=if(axis==null)next else JSONArray((captured?:sample).toString()).put(axis,next);vm.setPropertyValue(vm.selected,key,at,v,save)}
    Row(Modifier.fillMaxWidth().heightIn(min=56.dp),verticalAlignment=Alignment.CenterVertically) {
        TextButton(onClick={vm.pause();vm.property=key},modifier=Modifier.width(100.dp).heightIn(min=48.dp)){Text(label,color=if(vm.property==key)Accent else Ink,fontSize=13.sp)}
        NumericWheel(draft?:value,min,max,label,vm.editable(),Modifier.weight(1f).testTag("mask-wheel-${key.substringAfter("mask:")}-${axis?:0}"),
            onValueChange={next->if(captured==null){captured=sample;at=floor(vm.frame).toInt();vm.beginGesture()};draft=next;set(next,false)},
            onFinished={if(captured!=null)vm.endGesture{draft=null};captured=null},onCancelled={if(captured!=null)vm.cancelGesture();captured=null;draft=null},inertiaGroup=vm.gestureInertia)
        TextButton(onClick={input=true},enabled=vm.editable(),modifier=Modifier.widthIn(min=72.dp).heightIn(min=48.dp).testTag("mask-value-${key.substringAfter("mask:")}-${axis?:0}")){Text(String.format(java.util.Locale.US,"%.2f",draft?:value),fontSize=12.sp)}
    }
    if(input)InputDialog(label,value.toString(),{input=false},true){raw->val n=raw.toDoubleOrNull();if(n==null||!n.isFinite()||n !in min..max)vm.showOperationError("请输入 $min 到 $max 之间的数值")else {at=floor(vm.frame).toInt();set(n,true);input=false}}
}
