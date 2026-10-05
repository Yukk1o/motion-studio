package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import kotlin.math.*

internal fun effectPackages(vm:EditorViewModel)=vm.catalogue?.optJSONArray("packages").objects()
private fun compareVersions(a:String,b:String):Int {
    val left=a.substringBefore('+').split('-',limit=2);val right=b.substringBefore('+').split('-',limit=2)
    val l=left[0].split('.');val r=right[0].split('.')
    for(i in 0..2){val c=(l.getOrNull(i)?.toLongOrNull()?:0).compareTo(r.getOrNull(i)?.toLongOrNull()?:0);if(c!=0)return c}
    if(left.size!=right.size)return if(left.size==1)1 else -1
    if(left.size==1)return 0
    val lp=left[1].split('.');val rp=right[1].split('.')
    for(i in 0 until min(lp.size,rp.size)) {
        val ln=lp[i].toLongOrNull();val rn=rp[i].toLongOrNull()
        val c=when{ln!=null&&rn!=null->ln.compareTo(rn);ln!=null->-1;rn!=null->1;else->lp[i].compareTo(rp[i])}
        if(c!=0)return c
    }
    return lp.size.compareTo(rp.size)
}
/** Exact identities are used for saved instances; only new instances use newest enabled versions. */
internal fun availableEffects(vm:EditorViewModel):List<Pair<JSONObject,JSONObject>> {
    val latest=effectPackages(vm).filter{it.optBoolean("enabled")}.groupBy{it.getJSONObject("manifest").getString("id")}.values.map{versions->
        versions.maxWithOrNull(Comparator{a,b->compareVersions(a.getJSONObject("manifest").getString("version"),b.getJSONObject("manifest").getString("version"))})!!
    }
    return latest.flatMap{pkg->pkg.getJSONObject("manifest").getJSONArray("effects").objects().map{pkg to it}}
}
private fun effectLabel(vm:EditorViewModel,e:JSONObject)=vm.effectDefinition(e)?.optString("name")?:e.getString("effect")

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun EffectsPanel(vm:EditorViewModel,modifier:Modifier,onCurveMode:(Boolean)->Unit,onDismiss:()->Unit) {
    val objectId=vm.selected
    val instances=vm.layer(objectId)?.optJSONArray("effects").objects()
    var add by remember{mutableStateOf(false)}
    var instanceId by remember(objectId){mutableStateOf<Long?>(null)}
    var curveMode by remember(instanceId,vm.property){mutableStateOf(false)}
    var versionMenu by remember{mutableStateOf(false)}
    var upgrade by remember{mutableStateOf<JSONObject?>(null)}
    var numeric by remember{mutableStateOf<Triple<Long,JSONObject,Int>?>(null)}
    val current=instances.firstOrNull{it.getLong("id")==instanceId}
    LaunchedEffect(instances.map{it.getLong("id")}){if(instanceId!=null&&current==null)instanceId=null}
    LaunchedEffect(curveMode){onCurveMode(curveMode)}
    DisposableEffect(Unit){onDispose{onCurveMode(false)}}
    Box(modifier.background(Panel).clipToBounds()) {
        Column(Modifier.fillMaxSize().padding(horizontal=8.dp).testTag("effects-panel")) {
            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                if(current!=null||add)Tool(Icons.Default.ArrowBack,"返回效果列表"){instanceId=null;add=false;curveMode=false;vm.property="position"}
                Text(when{add->"添加效果";curveMode->"参数缓动曲线";current!=null->effectLabel(vm,current);else->"效果 · "+objectName(vm,objectId)},Modifier.weight(1f),color=Ink,fontSize=15.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                Box(Modifier.testTag("effects-play")){Tool(if(vm.playing)Icons.Default.Pause else Icons.Default.PlayArrow,if(vm.playing)"暂停效果预览"else"播放效果预览"){vm.togglePlay()}}
                if(current==null&&!add)TextButton(onClick={add=true},enabled=vm.editable(),modifier=Modifier.height(48.dp).testTag("effects-add")){Text("添加")}
                Tool(Icons.Default.Close,"关闭效果",action=onDismiss)
            }
            if(add)EffectCatalogue(vm,Modifier.weight(1f),onChoose={pkg,definition->
                val m=pkg.getJSONObject("manifest")
                vm.pluginOperation(JSONObject().put("op","add").put("object",objectId).put("plugin",m.getString("id")).put("version",m.getString("version"))
                    .put("hash",pkg.getString("hash")).put("effect",definition.getString("id")),true);add=false
            })else if(current==null) {
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                    if(instances.isEmpty())Text("尚未添加效果",Modifier.padding(vertical=20.dp),color=Muted)
                    instances.forEachIndexed{index,e->
                        Row(Modifier.fillMaxWidth().heightIn(min=64.dp).testTag("effect-instance-${e.getLong("id")}"),verticalAlignment=Alignment.CenterVertically) {
                            TextButton(onClick={instanceId=e.getLong("id")},modifier=Modifier.weight(1f)) {
                                Column(Modifier.fillMaxWidth()) {
                                    Text(effectLabel(vm,e),color=Ink,fontSize=15.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                                    Text(if(vm.effectDefinition(e)==null)"固定版本缺失 · ${e.getString("version")}"else e.getString("version"),color=Muted,fontSize=12.sp)
                                }
                            }
                            Switch(e.getBoolean("enabled"),{vm.effectAction(objectId,e.getLong("id"),"enable",JSONObject().put("enabled",it))},enabled=vm.editable())
                            var menu by remember(e.getLong("id")){mutableStateOf(false)}
                            Box {
                                Tool(Icons.Default.MoreVert,"效果操作"){menu=true}
                                DropdownMenu(menu,{menu=false}) {
                                    listOf("上移" to -1,"下移" to 1).forEach{(label,delta)->DropdownMenuItem(text={Text(label)},enabled=vm.editable()&&index+delta in instances.indices,onClick={menu=false;vm.effectAction(objectId,e.getLong("id"),"move",JSONObject().put("index",index+delta))})}
                                    DropdownMenuItem(text={Text("复制效果")},enabled=vm.editable(),onClick={menu=false;vm.effectAction(objectId,e.getLong("id"),"duplicate")})
                                    DropdownMenuItem(text={Text("删除效果")},enabled=vm.editable(),onClick={menu=false;vm.effectAction(objectId,e.getLong("id"),"remove")})
                                }
                            }
                        }
                        HorizontalDivider(color=Muted.copy(alpha=.12f))
                    }
                    vm.state.sample?.optJSONArray("effectErrors")?.let{a->for(i in 0 until a.length())Text(a.getString(i),color=MaterialTheme.colorScheme.error,fontSize=12.sp)}
                }
            }else {
                val definition=vm.effectDefinition(current)
                Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                    Box {
                        TextButton(onClick={versionMenu=true},enabled=vm.editable(),modifier=Modifier.testTag("effect-version")){Text("版本 "+current.getString("version"))}
                        DropdownMenu(versionMenu,{versionMenu=false}) {
                            effectPackages(vm).filter{it.optBoolean("enabled")&&it.getJSONObject("manifest").getString("id")==current.getString("plugin")}.forEach{pkg->
                                val m=pkg.getJSONObject("manifest")
                                DropdownMenuItem(text={Text(m.getString("version"))},enabled=m.getString("version")!=current.getString("version"),onClick={versionMenu=false;upgrade=pkg})
                            }
                        }
                    }
                    Text(if(definition==null)"版本缺失"else when(definition.optString("compatibility")){"verified"->"已验证";"unsupported"->"未支持";else->"近似效果"},Modifier.weight(1f),color=Muted,fontSize=11.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                    Tool(Icons.Default.Undo,"撤销效果编辑",vm.state.canUndo){vm.undo()}
                    Tool(Icons.Default.Redo,"重做效果编辑",vm.state.canRedo){vm.redo()}
                }
                if(curveMode) {
                    TextButton(onClick={curveMode=false}){Text("返回参数")}
                    CurveEditor(vm,Modifier.weight(1f).fillMaxWidth())
                    EffectTime(vm)
                }else Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                    val params=current.getJSONObject("params")
                    val definitions=definition?.getJSONArray("params").objects().associateBy{it.getString("id")}.orEmpty()
                    val ordered=if(definition!=null)definition.getJSONArray("params").objects().map{it.getString("id")}else params.keys().asSequence().toList()
                    ordered.forEach{paramId->
                        val saved=params.getJSONObject(paramId)
                        val desc=definitions[paramId]?:JSONObject().put("id",paramId).put("name",paramId).put("kind",saved.getString("kind")).put("min",saved.getDouble("min")).put("max",saved.getDouble("max"))
                        val instance=current.getLong("id")
                        val key="effect:$instance:$paramId"
                        val selected=vm.property==key
                        val track=vm.propertyTrack(objectId,key)
                        val animated=track?.optJSONArray("keys")?.length()?.let{it>0}==true
                        val enabled=vm.editable()&&saved.optBoolean("implemented",true)
                        val value=(vm.sampleValueFor(objectId,key) as? JSONArray)?:saved.optJSONObject("track")?.optJSONArray("value")?:JSONArray(listOf(0,0,0,0))
                        Column(Modifier.fillMaxWidth().padding(vertical=6.dp).testTag("effect-param-$paramId")) {
                            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                                TextButton(onClick={vm.chooseEffectParam(instance,paramId)},modifier=Modifier.weight(1f)) {
                                    Text(desc.optString("name",paramId),Modifier.fillMaxWidth(),color=if(selected)Accent else Ink,fontSize=14.sp)
                                }
                                if(saved.optBoolean("animatable"))TextButton(onClick={vm.chooseEffectParam(instance,paramId);vm.animate()},enabled=enabled,modifier=Modifier.height(48.dp).testTag("effect-animate-$paramId")) {Text(if(animated)"关闭动画"else"动画",fontSize=12.sp)}
                            }
                            if(!saved.optBoolean("implemented",true))Text("此参数暂未支持",color=Muted,fontSize=12.sp)
                            when(desc.getString("kind")) {
                                "curve"->EffectCurveObject(vm,objectId,instance,paramId,saved,enabled)
                                "bool"->Row(verticalAlignment=Alignment.CenterVertically){Switch(value.getDouble(0)>.5,{on->vm.effectAction(objectId,instance,"set",JSONObject().put("param",paramId).put("frame",floor(vm.frame).toInt()).put("value",JSONArray(value.toString()).put(0,if(on)1 else 0)))},enabled=enabled)}
                                "enum"->{
                                    var options by remember{mutableStateOf(false)}
                                    val list=desc.optJSONArray("options")?:JSONArray()
                                    val minimum=desc.getInt("min");val active=value.getInt(0)
                                    Box {
                                        TextButton(onClick={options=true},enabled=enabled,modifier=Modifier.heightIn(min=48.dp)){Text(list.optString(active-minimum,"值 $active"))}
                                        DropdownMenu(options,{options=false}){for(i in 0 until list.length())DropdownMenuItem(text={Text(list.getString(i))},onClick={options=false;vm.effectAction(objectId,instance,"set",JSONObject().put("param",paramId).put("frame",floor(vm.frame).toInt()).put("value",JSONArray(value.toString()).put(0,i+minimum)))})}
                                    }
                                }
                                else->{
                                    val dimensions=when(desc.getString("kind")){"vec2"->2;"vec3"->3;"color"->if(current.getString("plugin")=="com.motionstudio.effects.ae2021")3 else 4;else->1}
                                    if(desc.getString("kind")=="color")Box(Modifier.fillMaxWidth().height(8.dp).background(Color(value.getDouble(0).toFloat().coerceIn(0f,1f),value.getDouble(1).toFloat().coerceIn(0f,1f),value.getDouble(2).toFloat().coerceIn(0f,1f),1f)))
                                    repeat(dimensions){axis->
                                        EffectNumeric(vm,objectId,instance,desc,axis,value,enabled,onInput={numeric=Triple(instance,desc,axis)})
                                    }
                                }
                            }
                            if(selected&&animated) {
                                EffectTime(vm)
                                if(desc.getString("kind") !in listOf("bool","enum"))TextButton(onClick={curveMode=true},enabled=vm.easingSegment()!=null&&vm.easingSegment()!!.first.getInt("frame") in 0 until (vm.state.project?.optInt("frames")?:0),modifier=Modifier.height(48.dp).testTag("effect-easing")){Text("缓动曲线")}
                            }
                        }
                        HorizontalDivider(color=Muted.copy(alpha=.12f))
                    }
                    definition?.optJSONArray("known_differences")?.let{a->for(i in 0 until a.length())Text(a.getString(i),Modifier.padding(top=8.dp),color=Muted,fontSize=12.sp)}
                }
            }
        }
    }
    numeric?.let{(instance,desc,axis)->
        val paramId=desc.getString("id");val key="effect:$instance:$paramId"
        val original=JSONArray((vm.sampleValueFor(objectId,key) as JSONArray).toString());val at=floor(vm.frame).toInt()
        InputDialog(desc.getString("name"),original.getDouble(axis).toString(),onDismiss={numeric=null}){text->
            val number=text.toDoubleOrNull()
            if(number!=null&&number.isFinite()&&number in desc.getDouble("min")..desc.getDouble("max")) {
                vm.effectAction(objectId,instance,"set",JSONObject().put("param",paramId).put("frame",at).put("value",original.put(axis,number)));numeric=null
            }
        }
    }
    upgrade?.let{pkg->AlertDialog(onDismissRequest={upgrade=null},title={Text("切换效果版本")},text={Text("将重置此效果的全部参数、关键帧与随机种子。此操作可以撤销。")},confirmButton={TextButton(onClick={current?.let{e->val m=pkg.getJSONObject("manifest");vm.pluginOperation(JSONObject().put("op","upgrade").put("object",objectId).put("instance",e.getLong("id")).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("effect",e.getString("effect")),true)};upgrade=null}){Text("重置并切换")}},dismissButton={TextButton(onClick={upgrade=null}){Text("取消")}})}
}

@Composable private fun EffectNumeric(vm:EditorViewModel,objectId:Long,instance:Long,desc:JSONObject,axis:Int,value:JSONArray,enabled:Boolean,onInput:()->Unit) {
    val param=desc.getString("id");val minimum=desc.getDouble("min").toFloat();val maximum=desc.getDouble("max").toFloat()
    var draft by remember(instance,param,axis){mutableStateOf<Float?>(null)}
    var captured by remember{mutableStateOf<JSONArray?>(null)};var at by remember{mutableIntStateOf(0)}
    Column {
        Row(verticalAlignment=Alignment.CenterVertically) {
            Text(when(desc.getString("kind")){"vec2","vec3"->listOf("X","Y","Z")[axis];"color"->listOf("R","G","B","A")[axis];else->desc.optString("units")},color=Muted,fontSize=12.sp)
            Spacer(Modifier.weight(1f))
            TextButton(onClick=onInput,enabled=enabled,modifier=Modifier.heightIn(min=48.dp).testTag("effect-value-$param-$axis")){Text(String.format(Locale.US,"%.3f",(draft?:value.getDouble(axis).toFloat()))) }
        }
        if(maximum>minimum)StudioSlider(value=(draft?:value.getDouble(axis).toFloat()).coerceIn(minimum,maximum),valueRange=minimum..maximum,enabled=enabled,
            onValueChange={next->if(captured==null){captured=JSONArray(value.toString());at=floor(vm.frame).toInt();vm.beginGesture()};draft=next
                vm.effectAction(objectId,instance,"set",JSONObject().put("param",param).put("frame",at).put("value",JSONArray(captured.toString()).put(axis,next)),false)
            },onValueChangeFinished={if(captured!=null)vm.endGesture();draft=null;captured=null},modifier=Modifier.heightIn(min=48.dp).testTag("effect-slider-$param-$axis"))
    }
    DisposableEffect(instance,param,axis){onDispose{if(captured!=null)vm.cancelGesture()}}
}

@OptIn(ExperimentalFoundationApi::class)
@Composable private fun EffectTime(vm:EditorViewModel) {
    val frames=vm.state.project?.optInt("frames")?:1
    var editing by remember{mutableStateOf<Triple<Long,String,Int>?>(null)}
    var operation by remember{mutableStateOf<String?>(null)}
    var destination by remember{mutableStateOf("")}
    Column {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
            Tool(Icons.Default.SkipPrevious,"上一个效果关键帧",vm.keys().any{it.getInt("frame") in 0 until frames&&it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
            Tool(Icons.Default.Diamond,if(vm.currentKey()==null)"添加效果关键帧"else"删除效果关键帧",vm.editable()){vm.toggleKey()}
            Tool(Icons.Default.SkipNext,"下一个效果关键帧",vm.keys().any{it.getInt("frame") in 0 until frames&&it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
            Text("第 ${floor(vm.frame).toInt()} 帧",color=Muted,fontSize=12.sp)
        }
        StudioSlider(vm.frame.toFloat(),{vm.seek(it.roundToInt().toDouble())},valueRange=0f..(frames-1).coerceAtLeast(1).toFloat(),modifier=Modifier.testTag("effect-time"))
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            vm.keys().forEach{key->val f=key.getInt("frame")
                Text(if(f in 0 until frames)"◆ $f"else"$f · 合成外",color=if(f==floor(vm.frame).toInt())Accent else Muted,fontSize=12.sp,
                    modifier=Modifier.heightIn(min=48.dp).combinedClickable(onClick={if(f in 0 until frames)vm.seek(f.toDouble())},onLongClick={editing=Triple(vm.selected,vm.property,f);operation=null}).padding(12.dp).testTag("effect-key-$f"))
            }
        }
    }
    editing?.let{(objectId,property,from)->
        val to=destination.toIntOrNull()
        val occupied=vm.propertyTrack(objectId,property)?.optJSONArray("keys").objects().any{it.getInt("frame")==to}
        val valid=to!=null&&to in 0 until frames&&!occupied
        AlertDialog(onDismissRequest={editing=null},title={Text(if(operation==null)"关键帧 $from"else if(operation=="move")"移动关键帧"else"复制关键帧")},
            text={Column {
                if(operation==null){
                    TextButton(onClick={operation="move";destination=floor(vm.frame).toInt().toString()},enabled=vm.editable(),modifier=Modifier.testTag("effect-key-move")){Text("移动到…")}
                    TextButton(onClick={operation="copy";destination=floor(vm.frame).toInt().toString()},enabled=vm.editable(),modifier=Modifier.testTag("effect-key-copy")){Text("复制到…")}
                    TextButton(onClick={vm.deleteKeyFor(objectId,property,from,null);editing=null},enabled=vm.editable(),modifier=Modifier.testTag("effect-key-delete")){Text("删除关键帧")}
                }else OutlinedTextField(destination,{destination=it},label={Text("目标帧 · 0–${frames-1}")},singleLine=true,isError=!valid,
                    supportingText={if(!valid)Text(if(occupied)"目标帧已有关键帧"else"请输入合成范围内的整数帧")},modifier=Modifier.testTag("effect-key-destination"))
            }},confirmButton={if(operation!=null)TextButton(onClick={if(operation=="move")vm.moveKeyFor(objectId,property,from,to!!,null)else vm.copyKeyFor(objectId,property,from,to!!,null);editing=null},enabled=valid&&vm.editable(),modifier=Modifier.testTag("effect-key-confirm")){Text("确定")}},
            dismissButton={TextButton(onClick={editing=null}){Text("取消")}})
    }
}

@Composable private fun EffectCatalogue(vm:EditorViewModel,modifier:Modifier,onChoose:(JSONObject,JSONObject)->Unit) {
    var query by remember{mutableStateOf("")};var category by remember{mutableStateOf("全部")}
    val all=availableEffects(vm)
    val categories=listOf("全部")+listOf("调色","模糊与锐化","扭曲","光效","运动","风格化").filter{c->all.any{it.second.optString("category")==c}}+
        all.map{it.second.optString("category")}.distinct().filter{it !in listOf("调色","模糊与锐化","扭曲","光效","运动","风格化")}
    Column(modifier) {
        OutlinedTextField(query,{query=it},singleLine=true,placeholder={Text("搜索效果")},modifier=Modifier.fillMaxWidth().testTag("effect-search"))
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())){categories.forEach{c->TextButton(onClick={category=c},modifier=Modifier.height(48.dp).testTag("effect-category-$c").semantics{selected=category==c}){Text(c,color=if(category==c)Accent else Muted)}}}
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            all.filter{(_,e)->(category=="全部"||e.getString("category")==category)&&(query.isBlank()||e.getString("name").contains(query,true)||e.optString("english_name").contains(query,true))}.forEach{(pkg,e)->
                TextButton(onClick={onChoose(pkg,e)},enabled=e.optString("compatibility")!="unsupported",modifier=Modifier.fillMaxWidth().heightIn(min=64.dp).testTag("effect-add-${e.getString("id")}")) {
                    Column(Modifier.fillMaxWidth()){Text(e.getString("name"),color=Ink,fontSize=15.sp);Text(e.optString("english_name")+" · "+e.getString("category"),color=Muted,fontSize=12.sp)}
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun PluginsPanel(vm:EditorViewModel,onInstall:()->Unit,onDismiss:()->Unit) {
    var remove by remember{mutableStateOf<JSONObject?>(null)}
    LaunchedEffect(Unit){vm.refreshCatalogue()}
    ModalBottomSheet(onDismissRequest=onDismiss,sheetState=rememberModalBottomSheetState(skipPartiallyExpanded=true),containerColor=Panel) {
        Column(Modifier.fillMaxWidth().heightIn(max=560.dp).verticalScroll(rememberScrollState()).padding(16.dp).testTag("plugins-panel")) {
            Row(verticalAlignment=Alignment.CenterVertically){Text("效果包",Modifier.weight(1f),fontSize=20.sp);TextButton(onClick=onInstall,modifier=Modifier.height(48.dp).testTag("plugin-install")){Text("安装 .msfx")};Tool(Icons.Default.Close,"关闭效果包",action=onDismiss)}
            Text("工程使用固定版本。分享工程时，需要同时提供对应效果包。",color=Muted,fontSize=12.sp)
            effectPackages(vm).forEach{pkg->val m=pkg.getJSONObject("manifest")
                Row(Modifier.fillMaxWidth().heightIn(min=64.dp),verticalAlignment=Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)){Text(m.getString("name"),color=Ink,fontSize=15.sp);Text(m.getString("version"),color=Muted,fontSize=12.sp)}
                    Switch(pkg.getBoolean("enabled"),{on->vm.pluginOperation(JSONObject().put("op","enable").put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("enabled",on))})
                    if(m.getString("id")!="com.motionstudio.effects.ae2021")TextButton(onClick={remove=pkg}){Text("卸载")}
                }
            }
            vm.catalogue?.optJSONArray("errors")?.let{a->for(i in 0 until a.length())Text(a.getString(i),color=MaterialTheme.colorScheme.error,fontSize=12.sp)}
        }
    }
    remove?.let{pkg->AlertDialog(onDismissRequest={remove=null},title={Text("卸载效果包")},text={Text("使用此固定版本的效果将显示缺失，工程中的参数仍保留。")},confirmButton={TextButton(onClick={val m=pkg.getJSONObject("manifest");vm.pluginOperation(JSONObject().put("op","uninstall").put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")));remove=null}){Text("卸载")}},dismissButton={TextButton(onClick={remove=null}){Text("取消")}})}
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun StudioSlider(value:Float,onValueChange:(Float)->Unit,modifier:Modifier=Modifier,valueRange:ClosedFloatingPointRange<Float> =0f..1f,enabled:Boolean=true,onValueChangeFinished:(()->Unit)?=null) {
    Slider(value=value,onValueChange=onValueChange,valueRange=valueRange,enabled=enabled,onValueChangeFinished=onValueChangeFinished,modifier=modifier.heightIn(min=48.dp),
        thumb={Box(Modifier.size(8.dp).background(if(enabled)Accent else Muted,CircleShape))},
        track={state->Canvas(Modifier.fillMaxWidth().height(2.dp)) {
            drawLine(Muted.copy(alpha=.3f),Offset(0f,size.height/2),Offset(size.width,size.height/2),size.height)
            val fraction=((state.value-state.valueRange.start)/(state.valueRange.endInclusive-state.valueRange.start)).coerceIn(0f,1f)
            drawLine(if(enabled)Accent else Muted,Offset(0f,size.height/2),Offset(size.width*fraction,size.height/2),size.height)
        }})
}
