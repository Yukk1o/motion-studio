package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject

/** Reusable native page content; plugins supply slots, the host owns every widget. */
@Composable internal fun NativePluginEditorView(vm:EditorViewModel,host:PluginEditorHost,session:PluginEditorSession,modifier:Modifier,preview:@Composable (Modifier)->Unit) {
    val schema=session.definition.getJSONObject("native_editor")
    val state=host.state?:session.initialState
    val sections=schema.getJSONArray("sections").objects().filter{s->s.getJSONArray("slots").objects().any{it.getString("kind") !in setOf("preview","timeline")}}
    var tab by remember(session.token){mutableIntStateOf(0)}
    var selectedParam by remember(session.token){mutableStateOf(session.definition.getJSONArray("params").objects().firstOrNull()?.getString("id")?:"")}
    var editing by remember(session.token){mutableStateOf(false)}
    var error by remember(session.token){mutableStateOf<String?>(null)}
    var number by remember(session.token){mutableStateOf<Triple<String,String,(String)->Unit>?>(null)}
    val colors=remember(session.token){NativePluginColors(vm,host,session.token)}
    DisposableEffect(session.token){onDispose{if(host.session?.token!=session.token||vm.isClosed)vm.cancelEyedropper()}}
    val enabled=!state.optBoolean("locked")&&!editing&&!colors.busy
    val compact=LocalConfiguration.current.screenHeightDp<480
    fun edit(request:JSONObject) {
        if((host.state?:state).optBoolean("locked")||editing||colors.busy||colors.palette!=null)return
        vm.pause()
        editing=true;request.put("revision",(host.state?:state).getLong("revision"))
        host.request(request){reply->editing=false;error=if(reply.optBoolean("ok"))null else reply.optString("error")}
    }
    LaunchedEffect(session.token,selectedParam){vm.property="effect:${state.getLong("instance")}:$selectedParam"}
    fun close(commit:Boolean){vm.pause();colors.finish(commit){host.close(session.token,commit)}}
    BackHandler {close(false)}
    fun numeric(label:String,value:String,low:Double,high:Double,integer:Boolean=false,change:(Double)->Unit) {
        number=Triple(label,value){text->val v=text.toDoubleOrNull()
            if(v==null||!v.isFinite()||v !in low..high||(integer&&v%1.0!=0.0))error="请输入 $low～$high 之间的有效${if(integer)"整数"else"数值"}"
            else {change(v);number=null}}
    }
    Column(modifier.testTag("plugin-editor-native").then(if(compact)Modifier.verticalScroll(rememberScrollState())else Modifier)) {
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween) {
            Text(schema.getString("title"),Modifier.padding(12.dp),color=Ink)
            Text("原生设计器",Modifier.padding(12.dp),color=Muted)
        }
        val slots=schema.getJSONArray("sections").objects().flatMap{it.getJSONArray("slots").objects()}
        if(slots.any{it.getString("kind")=="preview"})preview(Modifier.fillMaxWidth().height(if(compact)96.dp else 180.dp).testTag("native-plugin-preview"))
        val definition=session.definition.getJSONArray("params").objects().firstOrNull{it.getString("id")==selectedParam}
        val keys=state.getJSONObject("params").optJSONObject(selectedParam)?.getJSONObject("track")?.getJSONArray("keys").objects().map{it.getInt("frame")+state.optInt("timeline_offset")}
        if(slots.any{it.getString("kind")=="timeline"})NativePluginTimeline(vm,state,definition?.getString("name")?:selectedParam,keys,enabled&&definition?.optBoolean("animatable")==true,
            transportEnabled=!editing&&!colors.busy,
            onPlay={colors.finish(true){vm.togglePlay(true)}},onSeek={at->colors.finish(true){vm.seek(at,true)}},
            onKey={colors.finish(true){edit(JSONObject().put("op","key").put("param",selectedParam))}})
        if(sections.isNotEmpty())ScrollableTabRow(tab.coerceAtMost(sections.lastIndex),edgePadding=0.dp) {
            sections.forEachIndexed{i,section->Tab(selected=tab==i,enabled=colors.palette==null&&!colors.busy,onClick={tab=i},modifier=Modifier.heightIn(min=48.dp).testTag("native-slot-tab-${section.getString("id")}"),text={Text(section.getString("title"))})}
        }
        val picker=colors.palette
        if(picker!=null)key(picker){ColorEditingPanel(picker,(if(compact)Modifier.height(360.dp)else Modifier.weight(1f)).fillMaxWidth(),colors::preview,{colors.finish(it)},colors::pick,vm.eyedropperActive)}
        else key("parameters",tab){Column((if(compact)Modifier else Modifier.weight(1f).verticalScroll(rememberScrollState())).fillMaxWidth().padding(12.dp).testTag("native-parameters"),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            sections.getOrNull(tab)?.getJSONArray("slots").objects().forEach{slot->when(slot.getString("kind")) {
                "parameters" -> slot.getJSONArray("params").let{ids->repeat(ids.length()){i->
                    val id=ids.getString(i);val p=session.definition.getJSONArray("params").objects().first{it.getString("id")==id};val value=state.getJSONObject("values").getJSONArray(id)
                    Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween) {
                        TextButton(onClick={selectedParam=id},modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("native-param-select-$id")){Text(p.getString("name")+p.optString("units").let{if(it.isEmpty())""else" · $it"},color=if(selectedParam==id)Accent else Ink)}
                        if(p.optBoolean("animatable"))TextButton(onClick={selectedParam=id;edit(JSONObject().put("op","key").put("param",id))},enabled=enabled,modifier=Modifier.heightIn(min=48.dp).testTag("native-param-key-$id")){Text("◆")}
                    }
                    val kind=p.getString("kind")
                    if(kind=="color") {
                        ColorParameterRow(p.getString("name"),"native-param-$id",Rgba.from(value),enabled,range=p.getDouble("min")..p.getDouble("max"),
                            onOpen={advanced->selectedParam=id;colors.open(p,advanced)},onPick={selectedParam=id;colors.open(p){colors.pick()}},
                            onChoose={chosen->selectedParam=id;colors.choose(p,chosen)})
                    }else if(kind=="enum"||kind=="bool") {
                        val options=if(kind=="bool")listOf("关闭","开启")else p.getJSONArray("options").let{a->List(a.length()){a.getString(it)}}
                        NativeChoice(options.mapIndexed{at,label->at.toLong() to label},value.getLong(0),enabled,"native-param-$id"){at->edit(JSONObject().put("op","set").put("param",id).put("value",JSONArray(value.toString()).put(0,at)))}
                    }else Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(4.dp)) {
                        val count=when(kind){"vec2"->2;"vec3"->3;else->1}
                        repeat(count){axis->val label=if(count==1)""else listOf("X","Y","Z")[axis]
                            OutlinedButton(onClick={numeric(p.getString("name")+" $label",value.getDouble(axis).toString(),p.getDouble("min"),p.getDouble("max")){v->
                                val rate=if(id=="rate")v else state.getJSONObject("values").optJSONArray("rate")?.getDouble(0)?:0.0
                                val life=if(id=="lifetime")v else state.getJSONObject("values").optJSONArray("lifetime")?.getDouble(0)?:0.0
                                if((id=="rate"||id=="lifetime")&&kotlin.math.ceil(rate*life)>20000)error="出生速率 × 寿命超过 20,000 个粒子"
                                else edit(JSONObject().put("op","set").put("param",id).put("value",JSONArray(value.toString()).put(axis,v)))
                            }},enabled=enabled,modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("native-param-$id-$axis"),contentPadding=PaddingValues(6.dp)){Text("$label ${"%.3g".format(value.getDouble(axis))}",maxLines=1,fontSize=14.sp)}
                        }
                    }
                }}
                "layer_source","image_sprite" -> {
                    val image=slot.getString("kind")=="image_sprite";val field=if(image)"sprite_asset"else"source_layer";val setting=state.getJSONObject("scene");val selected=setting.optLong(field,0)
                    Text(if(image)"粒子精灵"else"发射器路径来源",color=Ink)
                    val options=mutableListOf(0L to if(image)"柔光粒子"else"当前图层枢轴")
                    (if(image)state.optJSONArray("images")else state.optJSONArray("layers")).objects().filter{image||it.optBoolean("particle_source",true)}.forEach{o->options.add(o.getLong("id") to if(image)"图片 #${o.getLong("id")} · ${o.getInt("width")}×${o.getInt("height")}"else o.getString("name"))}
                    if(options.none{it.first==selected})options.add(selected to "已缺失 #$selected")
                    NativeChoice(options,selected,enabled,"native-slot-$field"){id->val next=JSONObject(setting.toString());if(id==0L)next.remove(field)else next.put(field,id);edit(JSONObject().put("op","scene").put("settings",next))}
                    Text(if(image)"使用已导入的 PNG 图片，保留透明轮廓和宽高比；多组粒子共享同一素材纹理。"else"跟随图层或 Null 的关键帧路径，旧粒子独立运动。",color=Muted,fontSize=12.sp)
                }
                "seed" -> OutlinedButton(onClick={numeric("随机种子",state.getLong("seed").toString(),0.0,4294967295.0,true){edit(JSONObject().put("op","seed").put("seed",it.toLong()))}},enabled=enabled,modifier=Modifier.heightIn(min=48.dp)){Text("随机种子 · ${state.getLong("seed")}")}
                "transform" -> listOf("position","rotation","scale").forEach{property->
                    Text(mapOf("position" to "发射器位置","rotation" to "旋转","scale" to "缩放")[property]!!,color=Ink)
                    val value=state.getJSONObject("transform_values").getJSONArray(property)
                    val limit=when(property){"position"->1e7;"rotation"->1e6;else->1e5}
                    Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(4.dp)){repeat(3){axis->OutlinedButton(onClick={numeric("$property ${listOf("X","Y","Z")[axis]}",value.getDouble(axis).toString(),-limit,limit){edit(JSONObject().put("op","transform").put("property",property).put("value",JSONArray(value.toString()).put(axis,it)))}},enabled=enabled,modifier=Modifier.weight(1f).heightIn(min=48.dp)){Text("${listOf("X","Y","Z")[axis]} ${"%.3g".format(value.getDouble(axis))}",maxLines=1)}}}
                }
                "note" -> Text(slot.getString("text"),color=Muted,fontSize=13.sp)
            }}
        }}
        error?.let{Text(it,Modifier.padding(horizontal=12.dp),color=MaterialTheme.colorScheme.error)}
        colors.error?.let{Text(it,Modifier.padding(horizontal=12.dp).testTag("native-color-error"),color=MaterialTheme.colorScheme.error)}
        if(picker==null)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.End) {
            TextButton(onClick={close(false)},modifier=Modifier.heightIn(min=48.dp).testTag("native-editor-cancel")){Text("取消")}
            TextButton(onClick={close(true)},enabled=!editing&&!colors.busy,modifier=Modifier.heightIn(min=48.dp).testTag("native-editor-done")){Text("完成")}
        }
    }
    number?.let{(title,initial,change)->InputDialog(title,initial,onDismiss={number=null},numeric=true,onConfirm=change)}
}

@Composable private fun NativePluginTimeline(vm:EditorViewModel,state:JSONObject,label:String,keys:List<Int>,enabled:Boolean,transportEnabled:Boolean,onPlay:()->Unit,onSeek:(Double)->Unit,onKey:()->Unit) {
    val frames=state.getInt("frames");val last=(frames-1).coerceAtLeast(1);val current=vm.frame.toInt()
    Column(Modifier.fillMaxWidth().padding(horizontal=12.dp).testTag("native-plugin-timeline")) {
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween) {
            TextButton(onClick=onPlay,enabled=transportEnabled,modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("native-plugin-play"),contentPadding=PaddingValues(4.dp)){Text(if(vm.playing)"暂停"else"播放",maxLines=1)}
            TextButton(onClick={keys.filter{it<current&&it>=0}.maxOrNull()?.let{onSeek(it.toDouble())}},enabled=transportEnabled&&keys.any{it<current&&it>=0},modifier=Modifier.weight(1f).heightIn(min=48.dp),contentPadding=PaddingValues(4.dp)){Text("上一键帧",maxLines=1)}
            TextButton(onClick=onKey,enabled=enabled,modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("native-plugin-key"),contentPadding=PaddingValues(4.dp)){Text(if(current in keys)"删除键帧"else"添加键帧",maxLines=1)}
            TextButton(onClick={keys.filter{it>current&&it<frames}.minOrNull()?.let{onSeek(it.toDouble())}},enabled=transportEnabled&&keys.any{it>current&&it<frames},modifier=Modifier.weight(1f).heightIn(min=48.dp),contentPadding=PaddingValues(4.dp)){Text("下一键帧",maxLines=1)}
        }
        Text("$label · 帧 $current / ${frames-1} · ${state.getInt("fps")} fps",color=Muted,fontSize=12.sp)
        Canvas(Modifier.fillMaxWidth().height(8.dp)){keys.filter{it in 0 until frames}.forEach{drawCircle(Color(0xFF58DCCA),3.dp.toPx(),Offset(it.toFloat()/last*size.width,size.height/2))}}
        Slider(value=vm.frame.toFloat().coerceIn(0f,last.toFloat()),onValueChange={onSeek(it.toInt().toDouble())},valueRange=0f..last.toFloat(),enabled=transportEnabled&&frames>1,modifier=Modifier.fillMaxWidth().heightIn(min=48.dp).testTag("native-plugin-playhead"))
    }
}

@Composable private fun NativeChoice(options:List<Pair<Long,String>>,selected:Long,enabled:Boolean,tag:String,onSelect:(Long)->Unit) {
    var expanded by remember(tag){mutableStateOf(false)}
    Box(Modifier.fillMaxWidth()) {
        OutlinedButton(onClick={expanded=true},enabled=enabled,modifier=Modifier.fillMaxWidth().heightIn(min=48.dp).testTag(tag)){Text(options.firstOrNull{it.first==selected}?.second?:"未选择")}
        DropdownMenu(expanded,{expanded=false}){options.forEach{(id,label)->DropdownMenuItem(text={Text(label)},onClick={expanded=false;onSelect(id)},modifier=Modifier.heightIn(min=48.dp))}}
    }
}
