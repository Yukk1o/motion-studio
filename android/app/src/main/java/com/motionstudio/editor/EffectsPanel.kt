package com.motionstudio.editor

import androidx.activity.compose.BackHandler
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
import androidx.compose.ui.platform.LocalDensity
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

@Composable internal fun EffectsPanel(vm:EditorViewModel,modifier:Modifier,backEnabled:Boolean=true,onCurveMode:(Boolean)->Unit,onDismiss:()->Unit) {
    val objectId=vm.selected
    val instances=vm.layer(objectId)?.optJSONArray("effects").objects()
    var add by remember(objectId){mutableStateOf(false)}
    var instanceId by remember(objectId){mutableStateOf(effectTarget(vm.property)?.first)}
    var curveMode by remember(instanceId,vm.property){mutableStateOf(false)}
    var details by remember(instanceId){mutableStateOf(false)}
    var information by remember(instanceId){mutableStateOf(false)}
    var upgrade by remember{mutableStateOf<JSONObject?>(null)}
    var numeric by remember{mutableStateOf<Triple<Long,JSONObject,Int>?>(null)}
    val current=instances.firstOrNull{it.getLong("id")==instanceId}
    val definition=current?.let{vm.effectDefinition(it)}
    val params=current?.getJSONObject("params")
    val definitions=definition?.getJSONArray("params").objects().associateBy{it.getString("id")}.orEmpty()
    val ordered=if(definition!=null)definition.getJSONArray("params").objects().map{it.getString("id")}else params?.keys()?.asSequence()?.toList().orEmpty()
    val target=effectTarget(vm.property)?.takeIf{it.first==instanceId}
    fun descriptor(param:String):JSONObject = definitions[param]?:params!!.getJSONObject(param).let{saved->
        JSONObject().put("id",param).put("name",param).put("kind",saved.getString("kind")).put("min",saved.getDouble("min")).put("max",saved.getDouble("max"))
    }
    fun back() {
        if(vm.pluginEditor.session!=null||vm.pluginEditor.loading)vm.pluginEditor.close()
        else if(curveMode)curveMode=false
        else {instanceId=null;add=false;vm.property="position"}
    }
    BackHandler(enabled=backEnabled&&(current!=null||add)){back()}
    LaunchedEffect(instances.map{it.getLong("id")}){if(instanceId!=null&&current==null)instanceId=null}
    LaunchedEffect(instanceId,ordered){if(current!=null&&target?.second !in ordered&&ordered.isNotEmpty())vm.chooseEffectParam(current.getLong("id"),ordered.first())}
    LaunchedEffect(curveMode){onCurveMode(curveMode)}
    DisposableEffect(Unit){onDispose{onCurveMode(false)}}
    Box(modifier.background(Panel).clipToBounds()) {
        Column(Modifier.fillMaxSize().padding(horizontal=8.dp).testTag("effects-panel")) {
            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                if(current!=null||add)Tool(Icons.Default.ArrowBack,if(curveMode)"返回效果参数"else"返回效果列表",action=::back)
                Text(when{vm.pluginEditor.session!=null->"专用编辑器";add->"添加效果";curveMode->"参数缓动";current!=null->effectLabel(vm,current);else->"效果 · "+objectName(vm,objectId)},Modifier.weight(1f),color=Ink,fontSize=15.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                if(current==null&&!add)TextButton(onClick={add=true},enabled=vm.editable(),modifier=Modifier.height(48.dp).testTag("effects-add")){Text("添加")}
                if(current!=null)Box {
                    Tool(Icons.Default.MoreHoriz,"效果详情与版本"){details=true}
                    DropdownMenu(details,{details=false}) {
                        if(definition?.optJSONObject("editor")!=null)DropdownMenuItem(text={Text("专用编辑器")},modifier=Modifier.testTag("open-plugin-editor"),onClick={details=false;vm.openPluginEditor(current.getLong("id"))})
                        vm.expressionTargetForCurrent()?.let{target->DropdownMenuItem(text={Text("当前参数表达式")},modifier=Modifier.testTag("open-effect-expression"),onClick={details=false;vm.openExpression(target)})}
                        DropdownMenuItem(text={Text("效果说明 · "+current.getString("version"))},onClick={details=false;information=true})
                        effectPackages(vm).filter{it.optBoolean("enabled")&&it.getJSONObject("manifest").getString("id")==current.getString("plugin")}.forEach{pkg->
                            val version=pkg.getJSONObject("manifest").getString("version")
                            if(version!=current.getString("version"))DropdownMenuItem(text={Text("切换版本 $version")},enabled=vm.editable(),onClick={details=false;upgrade=pkg})
                        }
                        DropdownMenuItem(text={Text("关闭当前参数动画")},enabled=vm.editable()&&vm.keys().isNotEmpty(),onClick={details=false;vm.animate();curveMode=false})
                    }
                }
                Tool(Icons.Default.Close,"关闭效果",action=onDismiss)
            }
            if(vm.pluginEditor.session!=null) {
                val session=vm.pluginEditor.session!!
                key(session.token){PluginEditorView(vm.pluginEditor,session,Modifier.weight(1f).fillMaxWidth().testTag("plugin-editor-webview"))}
            }else if(vm.pluginEditor.loading)Box(Modifier.weight(1f).fillMaxWidth(),contentAlignment=Alignment.Center){CircularProgressIndicator(color=Accent)}
            else if(add)EffectCatalogue(vm,Modifier.weight(1f),onChoose={pkg,e->
                val m=pkg.getJSONObject("manifest")
                vm.pluginOperation(JSONObject().put("op","add").put("object",objectId).put("plugin",m.getString("id")).put("version",m.getString("version"))
                    .put("hash",pkg.getString("hash")).put("effect",e.getString("id")),true);add=false
            })else if(current==null) {
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                    if(instances.isEmpty())Text("尚未添加效果",Modifier.padding(vertical=20.dp),color=Muted)
                    instances.forEachIndexed{index,e->
                        Row(Modifier.fillMaxWidth().heightIn(min=56.dp).testTag("effect-instance-${e.getLong("id")}"),verticalAlignment=Alignment.CenterVertically) {
                            TextButton(onClick={instanceId=e.getLong("id")},modifier=Modifier.weight(1f)) {
                                Column(Modifier.fillMaxWidth()) {
                                    Text(effectLabel(vm,e),color=Ink,fontSize=15.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                                    if(vm.effectDefinition(e)==null)Text("固定版本缺失 · ${e.getString("version")}",color=Muted,fontSize=12.sp)
                                }
                            }
                            Tool(if(e.getBoolean("enabled"))Icons.Default.Visibility else Icons.Default.VisibilityOff,if(e.getBoolean("enabled"))"停用效果"else"启用效果",vm.editable()) {
                                vm.effectAction(objectId,e.getLong("id"),"enable",JSONObject().put("enabled",!e.getBoolean("enabled")))
                            }
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
                    }
                    vm.state.sample?.optJSONArray("effectErrors")?.let{a->for(i in 0 until a.length())Text(a.getString(i),color=MaterialTheme.colorScheme.error,fontSize=12.sp)}
                }
            }else if(curveMode) {
                CurveEditor(vm,Modifier.weight(1f).fillMaxWidth())
            }else {
                vm.pluginEditor.error?.let{Text(it,color=MaterialTheme.colorScheme.error,fontSize=12.sp)}
                if(definition?.optJSONObject("editor")!=null)TextButton(onClick={vm.openPluginEditor(current.getLong("id"))},modifier=Modifier.height(48.dp).testTag("plugin-editor-open")){Text("打开专用编辑器")}
                target?.second?.takeIf{it in ordered}?.let{param->EffectControls(vm,descriptor(param),params!!.getJSONObject(param)){curveMode=true}}
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).testTag("effect-parameters")) {
                    ordered.forEach{paramId->
                        val saved=params!!.getJSONObject(paramId)
                        val desc=descriptor(paramId)
                        val instance=current.getLong("id")
                        val key="effect:$instance:$paramId"
                        val enabled=vm.editable()&&saved.optBoolean("implemented",true)
                        val value=(vm.sampleValueFor(objectId,key) as? JSONArray)?:saved.optJSONObject("track")?.optJSONArray("value")?:JSONArray(listOf(0,0,0,0))
                        val kind=desc.getString("kind")
                        fun select(){vm.chooseEffectParam(instance,paramId)}
                        Column(Modifier.fillMaxWidth().testTag("effect-param-$paramId")) {
                            when(kind) {
                                "float"->EffectNumeric(vm,objectId,instance,desc,0,value,enabled,if(paramId=="effect_opacity")"不透明度"else desc.optString("name",paramId),onSelect=::select,onInput={select();numeric=Triple(instance,desc,0)})
                                else->{
                                    Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                                        TextButton(onClick=::select,modifier=Modifier.weight(1f).testTag("effect-select-$paramId")){Text(desc.optString("name",paramId),Modifier.fillMaxWidth(),color=if(vm.property==key)Accent else Ink,fontSize=14.sp)}
                                        if(kind=="bool")Switch(value.getDouble(0)>.5,{on->select();vm.effectAction(objectId,instance,"set",JSONObject().put("param",paramId).put("frame",floor(vm.frame).toInt()).put("value",JSONArray(value.toString()).put(0,if(on)1 else 0)))},enabled=enabled)
                                        if(kind=="enum") {
                                            var options by remember{mutableStateOf(false)}
                                            val list=desc.optJSONArray("options")?:JSONArray()
                                            val minimum=desc.getInt("min");val active=value.getInt(0)
                                            Box {
                                                TextButton(onClick={select();options=true},enabled=enabled,modifier=Modifier.heightIn(min=48.dp)){Text(list.optString(active-minimum,"值 $active"))}
                                                DropdownMenu(options,{options=false}){for(i in 0 until list.length())DropdownMenuItem(text={Text(list.getString(i))},onClick={options=false;vm.effectAction(objectId,instance,"set",JSONObject().put("param",paramId).put("frame",floor(vm.frame).toInt()).put("value",JSONArray(value.toString()).put(0,i+minimum)))})}
                                            }
                                        }
                                    }
                                    if(kind=="curve")EffectCurveObject(vm,objectId,instance,paramId,saved,enabled)
                                    else if(kind !in listOf("bool","enum")) {
                                        val dimensions=when(kind){"vec2"->2;"vec3"->3;"color"->if(current.getString("plugin")=="com.motionstudio.effects.ae2021")3 else 4;else->1}
                                        repeat(dimensions){axis->
                                            val label=when(kind){"vec2","vec3"->listOf("X","Y","Z")[axis];"color"->listOf("R","G","B","A")[axis];else->desc.optString("units")}
                                            EffectNumeric(vm,objectId,instance,desc,axis,value,enabled,label,onSelect=::select,onInput={select();numeric=Triple(instance,desc,axis)})
                                        }
                                    }
                                }
                            }
                            if(!saved.optBoolean("implemented",true))Text("此参数暂未支持",color=Muted,fontSize=12.sp)
                        }
                    }
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
    if(information&&current!=null)AlertDialog(onDismissRequest={information=false},title={Text(effectLabel(vm,current))},text={Column(Modifier.heightIn(max=320.dp).verticalScroll(rememberScrollState())) {
        Text("版本 "+current.getString("version"),color=Muted)
        Text(if(definition==null)"固定版本缺失"else when(definition.optString("compatibility")){"verified"->"已验证";"unsupported"->"未支持";else->"近似效果"},color=Muted)
        definition?.optJSONArray("known_differences")?.let{a->for(i in 0 until a.length())Text(a.getString(i),Modifier.padding(top=8.dp),color=Muted,fontSize=12.sp)}
    }},confirmButton={TextButton(onClick={information=false}){Text("关闭")}})
    upgrade?.let{pkg->AlertDialog(onDismissRequest={upgrade=null},title={Text("切换效果版本")},text={Text("将重置此效果的全部参数、关键帧与随机种子。此操作可以撤销。")},confirmButton={TextButton(onClick={current?.let{e->val m=pkg.getJSONObject("manifest");vm.pluginOperation(JSONObject().put("op","upgrade").put("object",objectId).put("instance",e.getLong("id")).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("effect",e.getString("effect")),true)};upgrade=null}){Text("重置并切换")}},dismissButton={TextButton(onClick={upgrade=null}){Text("取消")}})}
}

@Composable private fun EffectControls(vm:EditorViewModel,desc:JSONObject,saved:JSONObject,onCurve:()->Unit) {
    val frames=vm.state.project?.optInt("frames")?:1
    val enabled=vm.editable()&&saved.optBoolean("implemented",true)&&saved.optBoolean("animatable")
    Row(Modifier.fillMaxWidth().heightIn(min=48.dp).testTag("effect-controls"),verticalAlignment=Alignment.CenterVertically) {
        Text(desc.getString("name"),Modifier.weight(1f),color=Muted,fontSize=12.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
        Tool(Icons.Default.SkipPrevious,"上一个效果关键帧",vm.keys().any{it.getInt("frame") in 0 until frames&&it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
        Box(Modifier.testTag("effect-animate-${desc.getString("id")}")) {
            Tool(Icons.Default.Diamond,if(vm.currentKey()==null)"添加效果关键帧"else"删除效果关键帧",enabled){vm.toggleKey()}
        }
        Tool(Icons.Default.SkipNext,"下一个效果关键帧",vm.keys().any{it.getInt("frame") in 0 until frames&&it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
        Box(Modifier.testTag("effect-easing")){Tool(Icons.Default.ShowChart,"编辑效果缓动曲线",enabled&&desc.getString("kind") !in listOf("bool","enum")&&vm.easingSegment()!=null,action=onCurve)}
    }
}

@Composable private fun EffectNumeric(vm:EditorViewModel,objectId:Long,instance:Long,desc:JSONObject,axis:Int,value:JSONArray,enabled:Boolean,label:String,onSelect:()->Unit,onInput:()->Unit) {
    val param=desc.getString("id");val minimum=desc.getDouble("min");val maximum=desc.getDouble("max")
    var draft by remember(instance,param,axis){mutableStateOf<Double?>(null)}
    var captured by remember{mutableStateOf<JSONArray?>(null)};var at by remember{mutableIntStateOf(0)}
    var adjusting by remember{mutableStateOf(false)}
    var gestureTicket by remember{mutableLongStateOf(0)}
    val fontScale=LocalDensity.current.fontScale
    Row(Modifier.fillMaxWidth().heightIn(min=(40.dp*fontScale).coerceAtLeast(56.dp)),verticalAlignment=Alignment.CenterVertically) {
        TextButton(onClick=onSelect,modifier=Modifier.width(96.dp).heightIn(min=48.dp).testTag(if(desc.getString("kind")=="float")"effect-select-$param"else"effect-axis-select-$param-$axis")) {
            Text(label,Modifier.fillMaxWidth(),color=if(vm.property=="effect:$instance:$param")Accent else Ink,fontSize=13.sp,maxLines=2,overflow=TextOverflow.Ellipsis)
        }
        if(maximum>minimum)NumericWheel(value=(draft?:value.getDouble(axis)).coerceIn(minimum,maximum),minimum=minimum,maximum=maximum,label=label,enabled=enabled,
            onValueChange={next->if(captured==null){captured=JSONArray(value.toString());at=floor(vm.frame).toInt();onSelect();vm.beginGesture();adjusting=true;gestureTicket++};draft=next
                vm.effectAction(objectId,instance,"set",JSONObject().put("param",param).put("frame",at).put("value",JSONArray(captured.toString()).put(axis,next)),false)
            },onFinished={val ticket=gestureTicket;if(captured!=null)vm.endGesture{if(!adjusting&&ticket==gestureTicket)draft=null};captured=null;adjusting=false},
            onCancelled={if(captured!=null)vm.cancelGesture();captured=null;draft=null;adjusting=false},modifier=Modifier.weight(1f).testTag("effect-wheel-$param-$axis"))
        else Spacer(Modifier.weight(1f))
        val valueWidth=(72.dp*fontScale).coerceIn(80.dp,120.dp)
        TextButton(onClick=onInput,enabled=enabled,contentPadding=PaddingValues(horizontal=4.dp),modifier=Modifier.width(valueWidth).heightIn(min=48.dp).testTag("effect-value-$param-$axis")) {
            Text(String.format(Locale.US,"%.3f",(draft?:value.getDouble(axis))),fontSize=12.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
    }
    DisposableEffect(instance,param,axis){onDispose{if(captured!=null)vm.cancelGesture()}}
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
