package com.motionstudio.editor

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject

/** Edits authored paths only; repeated instances retain one shared source. */
@Composable internal fun GroupPathControls(vm:EditorViewModel,keyPrefix:String,source:JSONObject,onChange:(JSONArray)->Unit) {
    val paths=source.optJSONArray("paths").objects()
    val initial=vm.property.takeIf{it.startsWith("$keyPrefix:node:")}
        ?.removePrefix("$keyPrefix:node:")?.split(':')
    var pathId by remember(vm.selected,keyPrefix){mutableLongStateOf(initial?.getOrNull(0)?.toLongOrNull()?:paths.firstOrNull()?.optLong("id")?:0)}
    var nodeId by remember(vm.selected,keyPrefix){mutableLongStateOf(initial?.getOrNull(1)?.toLongOrNull()?:0)}
    val path=paths.firstOrNull{it.optLong("id")==pathId}?:paths.firstOrNull()
    val nodes=path?.optJSONArray("nodes").objects()
    val node=nodes.firstOrNull{it.optLong("id")==nodeId}
    fun select(path:JSONObject,node:JSONObject?) {
        pathId=path.getLong("id");nodeId=node?.getLong("id")?:0
        vm.selectVectorTrack(if(node==null)"$keyPrefix:position"else"$keyPrefix:node:$pathId:$nodeId")
    }
    fun change(block:(MutableList<JSONObject>)->Unit) {
        val next=paths.map{JSONObject(it.toString())}.toMutableList();block(next)
        onChange(JSONArray(next))
    }
    Text("路径与节点",color=Ink,fontSize=14.sp)
    LazyRow(Modifier.fillMaxWidth().testTag("$keyPrefix-paths")) {
        items(paths,key={it.getLong("id")}){p->TextButton(onClick={select(p,null)},modifier=Modifier.heightIn(min=48.dp)
            .testTag("$keyPrefix-path-${p.getLong("id")}").semantics{selected=p.getLong("id")==path?.getLong("id")}){
            Text("路径 ${p.getLong("id")}",color=if(p.getLong("id")==path?.getLong("id"))Accent else Muted)}}
    }
    TextButton(onClick={
        val nextId=(paths.maxOfOrNull{it.getLong("id")}?:0)+1
        val next=JSONObject().put("id",nextId).put("closed",false).put("nodes",JSONArray())
        change{it.add(next)};select(next,null)
    },enabled=vm.editable()&&paths.size<64,modifier=Modifier.heightIn(min=48.dp).testTag("$keyPrefix-add-path")){Text("＋路径")}
    if(path==null)return
    LazyRow(Modifier.fillMaxWidth().testTag("$keyPrefix-nodes")) {
        items(nodes,key={it.getLong("id")}){n->TextButton(onClick={select(path,n)},modifier=Modifier.heightIn(min=48.dp)
            .testTag("$keyPrefix-node-${path.getLong("id")}-${n.getLong("id")}").semantics{selected=n==node}){
            Text("节点 ${n.getLong("id")}",color=if(n==node)Accent else Muted)}}
    }
    Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
        Text("闭合路径",Modifier.weight(1f),color=Ink)
        Switch(path.getBoolean("closed"),{closed->change{it.first{p->p.getLong("id")==path.getLong("id")}.put("closed",closed)}},
            enabled=vm.editable()&&nodes.size>=3,modifier=Modifier.testTag("$keyPrefix-close-path"))
    }
    TextButton(onClick={
        val nextId=(nodes.maxOfOrNull{it.getLong("id")}?:0)+1
        val next=JSONObject().put("id",nextId).put("geometry",JSONObject().put("value",JSONArray(listOf(0,0,0,0,0,0))).put("keys",JSONArray()))
        change{it.first{p->p.getLong("id")==path.getLong("id")}.getJSONArray("nodes").put(next)};select(path,next)
    },enabled=vm.editable()&&paths.sumOf{it.getJSONArray("nodes").length()}<2048,
        modifier=Modifier.heightIn(min=48.dp).testTag("$keyPrefix-add-node")){Text("＋节点")}
    if(node!=null) {
        val key="$keyPrefix:node:${path.getLong("id")}:${node.getLong("id")}"
        GroupPosition(vm,key,"节点位置")
        listOf("入柄 X","入柄 Y","出柄 X","出柄 Y").forEachIndexed{axis,label->VectorNumber(vm,key,label,-32768.0,32768.0,axis=axis+2)}
        TextButton(onClick={
            change{list->val p=list.first{it.getLong("id")==path.getLong("id")}
                p.put("nodes",JSONArray(p.getJSONArray("nodes").objects().filter{it.getLong("id")!=node.getLong("id")}))
                if(p.getJSONArray("nodes").length()<3)p.put("closed",false)}
            nodeId=0
            vm.selectVectorTrack("$keyPrefix:position")
        },enabled=vm.editable(),modifier=Modifier.heightIn(min=48.dp).testTag("$keyPrefix-delete-node")){Text("删除节点")}
    }else Text("选择节点后调整位置、切线和关键帧；预览显示整个效果链。",color=Muted,fontSize=12.sp)
    TextButton(onClick={change{it.removeAll{p->p.getLong("id")==path.getLong("id")}};nodeId=0;vm.selectVectorTrack("$keyPrefix:position")},enabled=vm.editable(),
        modifier=Modifier.heightIn(min=48.dp).testTag("$keyPrefix-delete-path")){Text("删除当前路径")}
}
