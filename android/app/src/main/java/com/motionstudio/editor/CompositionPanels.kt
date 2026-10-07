package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject

internal fun compositionErrorMessage(raw:String):String {
    val detail=runCatching{JSONObject(raw.substringAfter("composition_error:"))}.getOrNull()?:return raw
    return when(detail.optString("code")) {
        "noncontiguous_selection"->"请选择层级相邻的图层后再预合成。"
        "external_parent"->"请把关联的父子图层一起选中，解除选区外的绑定后再预合成。"
        "external_reference"->"选区外的效果正在引用这些图层，请先调整引用。"
        "expression_context"->"有表达式依赖图层序号，请先调整或禁用表达式。"
        "unsupported_mode"->"当前选区包含暂不支持预合成的空间内容。"
        "stale_revision"->"工程已变化，请重新预览设置影响后确认。"
        "keyframe_collision"->"帧率变化会合并关键帧，请选择保留帧编号或调整关键帧。"
        "composition_in_use"->"此合成仍被图层引用，请先删除引用图层。"
        "resource_limit"->"合成数量、嵌套深度或资源使用已达到上限。"
        "cycle"->"此引用会形成循环，请选择其他合成。"
        "context_busy"->"请先结束当前拖动或关闭插件编辑器。"
        else->detail.optString("message","合成操作失败")
    }
}

@Composable internal fun CompositionBreadcrumbs(vm:EditorViewModel,modifier:Modifier) {
    val names=vm.state.sample?.optJSONArray("compositions").objects().associate{it.getString("id") to it.getString("name")}
    Row(modifier.horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically) {
        vm.compositionPath.forEachIndexed{i,id->
            if(i>0)Text("›",color=Muted)
            TextButton(onClick={vm.openComposition(id,vm.compositionPath.take(i+1))},enabled=i<vm.compositionPath.lastIndex&&!vm.state.busy,modifier=Modifier.heightIn(min=48.dp).testTag("composition-breadcrumb-$i")) {
                Text(names[id]?:vm.state.project?.optString("name")?:"合成",fontSize=13.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun CompositionLibrary(vm:EditorViewModel,onDismiss:()->Unit) {
    val nodes=vm.state.sample?.optJSONArray("compositions").objects()
    var candidates by remember(vm.compositionId){mutableStateOf<Set<String>>(emptySet())}
    var creating by remember{mutableStateOf(false)}
    var deleting by remember{mutableStateOf<JSONObject?>(null)}
    var deleteInfo by remember{mutableStateOf<JSONObject?>(null)}
    LaunchedEffect(vm.compositionId,vm.state.sample?.optLong("revision")){vm.compositionQuery("reference_candidates"){data->candidates=data.getJSONArray("compositions").let{a->(0 until a.length()).map{a.getString(it)}.toSet()}}}
    ModalBottomSheet(onDismissRequest=onDismiss,containerColor=Panel,sheetState=rememberModalBottomSheetState(skipPartiallyExpanded=true)) {
        Column(Modifier.fillMaxWidth().heightIn(max=560.dp).padding(horizontal=16.dp).padding(bottom=16.dp).testTag("composition-library")) {
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically){Text("合成",Modifier.weight(1f),fontSize=20.sp);TextButton(onClick={creating=true},enabled=nodes.size<32,modifier=Modifier.testTag("create-child-composition")){Text("新建")}}
            Column(Modifier.weight(1f,false).verticalScroll(rememberScrollState())){nodes.forEach{node->val id=node.getString("id")
                Column(Modifier.fillMaxWidth().padding(vertical=8.dp)) {
                    Text(node.getString("name")+if(id==vm.compositionId)" · 当前"else"",color=if(id==vm.compositionId)Accent else Ink)
                    Text("${node.getInt("width")} × ${node.getInt("height")} · ${node.getInt("fps")} fps · ${node.getInt("frames")} 帧",color=Muted,fontSize=12.sp)
                    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                        TextButton(onClick={vm.openComposition(id);onDismiss()},enabled=id!=vm.compositionId,modifier=Modifier.testTag("open-composition-$id")){Text("编辑")}
                        TextButton(onClick={vm.compositionAction(JSONObject().put("kind","reference").put("target",id).put("at_frame",vm.frame.toInt()));onDismiss()},enabled=id in candidates,modifier=Modifier.testTag("reference-composition-$id")){Text("加入当前合成")}
                        if(!node.optBoolean("main")&&id!=vm.compositionId)TextButton(onClick={deleting=node;deleteInfo=null;vm.compositionQuery("delete_check",id){deleteInfo=it}},modifier=Modifier.testTag("delete-composition-$id")){Text("删除")}
                    }
                }
            }}
        }
    }
    if(creating)NewProjectDialog(onDismiss={creating=false},title="新建子合成"){w,h,fps,name,frames->creating=false;vm.compositionAction(JSONObject().put("kind","create").put("settings",JSONObject().put("name",name.ifBlank{"子合成"}).put("width",w).put("height",h).put("fps",fps).put("frames",frames)))}
    deleting?.let{node->AlertDialog(onDismissRequest={deleting=null},title={Text("删除 ${node.getString("name")}？")},text={Text(if(deleteInfo==null)"正在检查引用…"else if(deleteInfo?.optBoolean("can_delete")==true)"删除空闲合成，可撤销恢复。"else"此合成仍被引用，无法删除。")},confirmButton={TextButton(onClick={vm.compositionAction(JSONObject().put("kind","delete").put("target",node.getString("id")));deleting=null},enabled=deleteInfo?.optBoolean("can_delete")==true){Text("删除")}},dismissButton={TextButton(onClick={deleting=null}){Text("取消")}})}
}

@Composable internal fun CompositionSettings(vm:EditorViewModel,onDismiss:()->Unit) {
    val p=vm.state.project?:return
    var name by remember{mutableStateOf(p.getString("name"))}
    var width by remember{mutableStateOf(p.getInt("width").toString())};var height by remember{mutableStateOf(p.getInt("height").toString())}
    var fps by remember{mutableStateOf(p.getInt("fps").toString())};var seconds by remember{mutableStateOf((p.getInt("frames").toDouble()/p.getInt("fps")).toString())}
    var timing by remember{mutableStateOf("preserve_seconds")};var trim by remember{mutableStateOf(false)}
    var preview by remember{mutableStateOf<JSONObject?>(null)};var proposed by remember{mutableStateOf<JSONObject?>(null)}
    val w=width.toIntOrNull();val h=height.toIntOrNull();val rate=fps.toIntOrNull();val frames=rate?.takeIf{it in 1..240}?.let{compositionFrames(seconds,it)}
    fun invalidated(){preview=null;proposed=null}
    AlertDialog(onDismissRequest=onDismiss,title={Text("合成设置")},text={Column(Modifier.heightIn(max=440.dp).verticalScroll(rememberScrollState()).testTag("composition-settings")) {
        OutlinedTextField(name,{name=it;invalidated()},label={Text("名称")},singleLine=true,modifier=Modifier.fillMaxWidth())
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(width,{width=it;invalidated()},label={Text("宽度")},singleLine=true,isError=w==null||w !in 1..8192,modifier=Modifier.weight(1f))
            OutlinedTextField(height,{height=it;invalidated()},label={Text("高度")},singleLine=true,isError=h==null||h !in 1..8192,modifier=Modifier.weight(1f))
        }
        OutlinedTextField(fps,{fps=it;invalidated()},label={Text("帧率 · 1–240")},singleLine=true,isError=rate==null||rate !in 1..240,modifier=Modifier.fillMaxWidth().testTag("composition-settings-fps"))
        OutlinedTextField(seconds,{seconds=it;invalidated()},label={Text("时长（秒）")},singleLine=true,isError=frames==null,modifier=Modifier.fillMaxWidth().testTag("composition-settings-duration"))
        Row(Modifier.horizontalScroll(rememberScrollState())){listOf("preserve_seconds" to "保留播放秒数","preserve_frames" to "保留帧编号").forEach{(id,label)->TextButton(onClick={timing=id;invalidated()}){Text(label,color=if(timing==id)Accent else Muted)}}}
        Row(verticalAlignment=Alignment.CenterVertically){Checkbox(trim,{trim=it;invalidated()});Text("允许裁剪超出新时长的内容",fontSize=12.sp)}
        Text("尺寸变化保留像素位置；延长时长不会自动拉长图层。",color=Muted,fontSize=12.sp)
        preview?.let{result->
            if(!result.optBoolean("valid"))Text(compositionErrorMessage(result.optString("error")),color=MaterialTheme.colorScheme.error)
            else {
                Text("设置影响",color=Accent)
                val impacts=result.optJSONArray("results").objects().flatMap{it.optJSONObject("result")?.optJSONArray("impacts").objects()}
                if(impacts.isEmpty())Text("没有需要重定时或裁剪的内容。",fontSize=13.sp)
                impacts.forEach{Text(compositionImpact(it),fontSize=12.sp)}
            }
        }
    }},confirmButton={
        if(preview?.optBoolean("valid")==true&&proposed!=null)TextButton(onClick={vm.applyCompositionSettings(proposed!!,preview!!.getLong("expected_revision"));onDismiss()},modifier=Modifier.testTag("apply-composition-settings")){Text("确认应用")}
        else TextButton(onClick={val settings=JSONObject().put("name",name).put("width",w).put("height",h).put("fps",rate).put("frames",frames).put("timing",timing).put("shorten",if(trim)"trim"else"reject");proposed=settings;vm.compositionQuery("settings_preview",fields=JSONObject().put("settings",settings)){preview=it}},enabled=w!=null&&h!=null&&w in 1..8192&&h in 1..8192&&frames!=null,modifier=Modifier.testTag("preview-composition-settings")){Text("预览影响")}
    },dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}

private fun compositionImpact(impact:JSONObject):String {
    val label=when(impact.optString("kind")) {
        "keyframe"->"关键帧 ${impact.optInt("before")} → ${impact.optInt("after")} 帧"
        "clip"->"裁剪片段至 ${impact.optInt("new_end")} 帧"
        "removed_layer"->"移除新时长以外的图层"
        "removed_keyframe"->"移除新时长以外的关键帧"
        "removed_camera_keyframe"->"移除新时长以外的摄影机关键帧"
        "reference"->"同步其他合成中引用图层的尺寸"
        else->"调整内容时间"
    }
    return label+impact.optLong("object").takeIf{it!=0L}?.let{" · 图层 $it"}.orEmpty()
}

@Composable internal fun CompositionClipPanel(vm:EditorViewModel,modifier:Modifier) {
    val clip=vm.layer(vm.selected)?.optJSONObject("content")?.optJSONObject("clip")?:return
    Column(modifier.background(Panel).testTag("composition-clip-panel")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp).padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
            Text("子合成片段",Modifier.weight(1f),color=Ink)
            TextButton(onClick=vm::openSelectedComposition,enabled=!vm.state.busy,modifier=Modifier.testTag("edit-child-composition")){Text("进入编辑")}
            TextButton(onClick=vm::closeWorkspace,modifier=Modifier.testTag("close-composition-clip")){Text("完成")}
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal=8.dp)) {
            CompositionClipNumber(vm,clip,"source_start_frame","源起点（帧）",-36000.0,36000.0)
            CompositionClipNumber(vm,clip,"volume","音量",0.0,4.0)
            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically){
                Text("静音",Modifier.weight(1f));Switch(clip.optBoolean("muted"),{next->vm.compositionAction(clipAction(vm.selected,clip).put("muted",next))},enabled=vm.editable(),modifier=Modifier.testTag("composition-clip-muted"))
            }
            Text("源起点按子合成帧率计算；移动或裁剪当前片段仍在时间轴操作。",fontSize=12.sp,color=Muted)
        }
    }
}

private fun clipAction(id:Long,clip:JSONObject)=JSONObject().put("kind","set_clip").put("object",id).put("source_start_frame",clip.optInt("source_start_frame")).put("volume",clip.optDouble("volume",1.0)).put("muted",clip.optBoolean("muted"))

@Composable private fun CompositionClipNumber(vm:EditorViewModel,clip:JSONObject,key:String,label:String,min:Double,max:Double) {
    val id=vm.selected
    var captured by remember(vm.compositionId,id,key){mutableStateOf<JSONObject?>(null)}
    var draft by remember(vm.compositionId,id,key){mutableStateOf<Double?>(null)}
    var input by remember{mutableStateOf(false)}
    val value=clip.optDouble(key,if(key=="volume")1.0 else 0.0)
    fun command(next:Double)=clipAction(id,captured?:clip).put(key,if(key=="source_start_frame")kotlin.math.round(next).toInt() else next)
    Row(Modifier.fillMaxWidth().heightIn(min=56.dp),verticalAlignment=Alignment.CenterVertically) {
        Text(label,Modifier.width(100.dp),fontSize=13.sp)
        NumericWheel(draft?:value,min,max,label,vm.editable(),Modifier.weight(1f).testTag("composition-clip-wheel-$key"),
            onValueChange={next->if(captured==null)captured=JSONObject(clip.toString());draft=if(key=="source_start_frame")kotlin.math.round(next)else next},
            onFinished={draft?.let{vm.compositionAction(command(it))};draft=null;captured=null},onCancelled={draft=null;captured=null},inertiaGroup=vm.gestureInertia)
        TextButton(onClick={vm.gestureInertia.stop();input=true},enabled=vm.editable(),modifier=Modifier.widthIn(min=72.dp).testTag("composition-clip-value-$key")){Text(String.format(java.util.Locale.US,if(key=="volume")"%.2f"else"%.0f",draft?:value),fontSize=12.sp)}
    }
    if(input)InputDialog(label,value.toString(),{input=false},true){raw->val next=raw.toDoubleOrNull();if(next!=null&&next.isFinite()&&next in min..max){vm.compositionAction(command(next));input=false}else vm.showOperationError("请输入 $min 到 $max 之间的数值")}
}
