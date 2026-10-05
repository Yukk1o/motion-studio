package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

private val easeNames=listOf("linear" to "线性","in" to "缓入","out" to "缓出","in_out" to "缓入缓出","hold" to "保持")
private fun defaultCurve(kind:String,space:String):JSONObject {
    val velocity=space=="velocity"
    val shape=JSONObject().put("kind",kind)
    when(kind) {
        "quadratic"->shape.put("control",JSONArray(listOf(.5,if(velocity)3.0 else .0)))
        "cubic"->shape.put("control1",JSONArray(listOf(.25,if(velocity)2.0 else .0)))
            .put("control2",JSONArray(listOf(.75,if(velocity)2.0 else 1.0)))
        "elastic"->shape.put("oscillations",2.5).put("damping",6.0)
    }
    if(kind!="elastic")shape.put("start",0.0).put("end",if(velocity)0.0 else 1.0)
    return JSONObject().put("ease","linear").put("curve",JSONObject().put("space",space).put("shape",shape))
}
private fun graphData(easing:String):JSONObject?=runCatching {
    JSONObject(NativeBridge.curveGraph(easing)).takeIf{it.optBoolean("ok")}?.getJSONObject("data")
}.getOrNull()
/** Presets keep their compact representation until the first handle drag. The
 * Bezier control points reproduce the core's polynomial progress exactly. */
private fun handleDefinition(easing:JSONObject,view:String):JSONObject {
    if(easing.has("curve")||view!="progress"||easing.optString("ease")=="hold")return easing
    val ease=easing.optString("ease","linear")
    val kind=if(ease in listOf("in","out"))"quadratic"else"cubic"
    return defaultCurve(kind,"progress").also{definition->
        val shape=definition.getJSONObject("curve").getJSONObject("shape")
        when(ease) {
            "in"->shape.put("control",JSONArray(listOf(.5,0.0)))
            "out"->shape.put("control",JSONArray(listOf(.5,1.0)))
            "in_out"->shape.put("control1",JSONArray(listOf(1.0/3,0.0))).put("control2",JSONArray(listOf(2.0/3,1.0)))
            else->shape.put("control1",JSONArray(listOf(1.0/3,1.0/3))).put("control2",JSONArray(listOf(2.0/3,2.0/3)))
        }
    }
}
private fun handles(easing:JSONObject,view:String,points:JSONArray?,scale:Float):List<Pair<String,Offset>> {
    val curve=easing.optJSONObject("curve")?:return emptyList()
    val shape=curve.getJSONObject("shape")
    if(shape.optString("kind")=="elastic"&&points!=null) {
        val oscillations=shape.getDouble("oscillations").toFloat()
        fun point(t:Float):Offset {
            val index=t*(points.length()-1);val left=floor(index).toInt();val right=min(left+1,points.length()-1)
            val a=points.getJSONObject(left).getDouble(view).toFloat();val b=points.getJSONObject(right).getDouble(view).toFloat()
            return Offset(t,(a+(b-a)*(index-left))/scale)
        }
        // One two-dimensional handle lies on the core's first sampled lobe.
        // Horizontal movement changes frequency, vertical movement changes damping.
        return listOf("elastic" to point((if(view=="progress").5f else .25f)/oscillations))
    }
    if(curve.optString("space","progress")!=view)return emptyList()
    val names=when(shape.getString("kind")){"quadratic"->listOf("control");"cubic"->listOf("control1","control2");else->emptyList()}
    return names.map{key->val p=shape.getJSONArray(key);key to Offset(p.getDouble(0).toFloat(),p.getDouble(1).toFloat())}
}

@Composable internal fun CurveEditor(vm:EditorViewModel,modifier:Modifier) {
    var view by remember(vm.selected,vm.property){mutableStateOf("progress")}
    var parameters by remember{mutableStateOf(false)}
    var copied by remember{mutableStateOf(false)}
    var draft by remember(vm.selected,vm.property){mutableStateOf<String?>(null)}
    var dragging by remember{mutableStateOf(false)}
    var frozenRange by remember{mutableStateOf<Pair<Float,Float>?>(null)}
    val segment=vm.easingSegment()
    val committed=vm.easingDefinition()?.toString()?:JSONObject().put("ease","linear").toString()
    LaunchedEffect(committed){if(!dragging)draft=null}
    LaunchedEffect(Unit){vm.refreshCurveClipboard()}
    val definition=JSONObject(draft?:committed)
    val data=remember(draft?:committed){graphData(draft?:committed)}
    val points=data?.optJSONArray("points")
    val scale=data?.optDouble("definitionScale",1.0)?.toFloat()?:1f
    val controlDefinition=handleDefinition(definition,view)
    val controlPoints=handles(controlDefinition,view,points,scale)
    val samples=points?.let{a->(0 until a.length()).map{a.getJSONObject(it).getDouble(view).toFloat()}}?:listOf(0f,1f)
    val ordinates=samples+controlPoints.map{it.second.y*scale}+listOf(0f,1f)
    val low=min(0f,ordinates.minOrNull()?:0f)
    val high=max(1f,ordinates.maxOrNull()?:1f)
    val range=frozenRange?:((low-.08f*(high-low)) to (high+.08f*(high-low)))
    val liveDefinition by rememberUpdatedState(controlDefinition.toString())
    val liveRange by rememberUpdatedState(range)
    val liveScale by rememberUpdatedState(scale)
    val liveHandles by rememberUpdatedState(controlPoints)
    Column(modifier) {
        Row(Modifier.fillMaxWidth().height(48.dp).horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically) {
            listOf("progress" to "进度","velocity" to "速度").forEach{(key,label)->TextButton(onClick={view=key;copied=false},
                modifier=Modifier.height(48.dp).testTag("curve-view-"+key).semantics{selected=view==key}){Text(label,color=if(view==key)Accent else Muted,fontSize=14.sp)}}
            Spacer(Modifier.weight(1f))
            Tool(Icons.Default.ContentCopy,if(copied)"曲线已复制"else"复制曲线",segment!=null){vm.copyCurve();copied=true}
            Tool(Icons.Default.ContentPaste,"粘贴曲线",segment!=null&&vm.editable()&&vm.curveClipboard!=null){vm.pasteCurve();copied=false}
            Tool(Icons.Default.Tune,"曲线参数",segment!=null&&vm.editable()){parameters=true}
        }
        Text(segment?.let{"第 ${it.first.getInt("frame")} → ${it.second.getInt("frame")} 帧 · "+if(view=="progress")"动画进度"else"速度 · 1 为匀速"}?:"此属性还没有可编辑的关键帧区间",
            modifier=Modifier.padding(horizontal=12.dp),color=Muted,fontSize=12.sp,lineHeight=20.sp,maxLines=1)
        Box(Modifier.weight(1f).fillMaxWidth().padding(12.dp).background(Background,RoundedCornerShape(12.dp))) {
        Canvas(Modifier.fillMaxSize().padding(horizontal=20.dp,vertical=16.dp).testTag("easing-graph")
            .pointerInput(vm.selected,vm.property,segment?.first?.optInt("frame"),view) {
                fun pixel(point:Offset,bounds:Pair<Float,Float>,factor:Float)=Offset(point.x*size.width,(bounds.second-point.y*factor)/(bounds.second-bounds.first)*size.height)
                awaitEachGesture {
                    val down=awaitFirstDown(requireUnconsumed=false)
                    val nearest=liveHandles.minByOrNull{(_,point)->(pixel(point,liveRange,liveScale)-down.position).getDistance()}
                    if(segment==null||!vm.editable()||nearest==null||(pixel(nearest.second,liveRange,liveScale)-down.position).getDistance()>24.dp.toPx())return@awaitEachGesture
                    down.consume()
                    val (key,origin)=nearest
                    var movement=Offset.Zero
                    val dragDefinition=liveDefinition;val dragScale=liveScale;val dragBounds=liveRange
                    frozenRange=dragBounds;dragging=true;vm.beginGesture()
                    var completed=false
                    try { completed=drag(down.id){change->
                        movement+=change.positionChange();change.consume()
                        val p=Offset((origin.x+movement.x/size.width).coerceIn(0f,1f),
                            (origin.y-movement.y/size.height*(dragBounds.second-dragBounds.first)/dragScale).coerceIn(-8f,8f))
                        val edited=JSONObject(dragDefinition)
                        val shape=edited.getJSONObject("curve").getJSONObject("shape")
                        if(shape.optString("kind")=="elastic") {
                            val phase=if(view=="progress").5 else .25
                            val time=(origin.x+movement.x/size.width).toDouble().coerceIn(phase/8,phase/.5)
                            shape.put("oscillations",(phase/time).coerceIn(.5,8.0))
                            shape.put("damping",(shape.getDouble("damping")+movement.y/size.height*20.0).coerceIn(.5,20.0))
                        } else shape.put(key,JSONArray(listOf(p.x.toDouble(),p.y.toDouble())))
                        if(graphData(edited.toString())!=null){draft=edited.toString();vm.setCurve(edited,false)}
                    }} finally {
                        if(completed)vm.endGesture()else{vm.cancelGesture();draft=null}
                        dragging=false;frozenRange=null
                    }
                }
            }) {
            fun pixel(x:Float,y:Float)=Offset(x*size.width,(range.second-y)/(range.second-range.first)*size.height)
            for(i in 0..4) {
                drawLine(Muted.copy(alpha=.16f),Offset(0f,size.height*i/4),Offset(size.width,size.height*i/4),1f)
                drawLine(Muted.copy(alpha=.16f),Offset(size.width*i/4,0f),Offset(size.width*i/4,size.height),1f)
            }
            for(y in listOf(0f,1f))drawLine(Muted.copy(alpha=.35f),pixel(0f,y),pixel(1f,y),1f)
            val path=Path()
            samples.forEachIndexed{i,y->val p=pixel(i.toFloat()/(samples.size-1),y);if(i==0)path.moveTo(p.x,p.y)else path.lineTo(p.x,p.y)}
            drawPath(path,if(segment!=null)Accent else Muted.copy(alpha=.2f),style=Stroke(2.dp.toPx()))
            controlPoints.forEachIndexed{i,(_,point)->
                val control=pixel(point.x,point.y*scale)
                val shape=controlDefinition.getJSONObject("curve").getJSONObject("shape")
                if(shape.optString("kind")!="elastic") {
                    val start=pixel(0f,shape.optDouble("start",0.0).toFloat()*scale)
                    val end=pixel(1f,shape.optDouble("end",1.0).toFloat()*scale)
                    val anchors=if(shape.optString("kind")=="quadratic")listOf(start,end)else listOf(if(i==0)start else end)
                    anchors.forEach{anchor->drawLine(Muted.copy(alpha=.65f),anchor,control,1.dp.toPx())}
                }
                drawCircle(if(i==0)Accent else Color(0xFF83BAEB),3.dp.toPx(),control)
            }
        }
        if(segment==null)Column(Modifier.align(Alignment.Center).background(Background.copy(alpha=.95f)).padding(16.dp),horizontalAlignment=Alignment.CenterHorizontally) {
            Icon(editorIcon(Icons.Default.Diamond),null,tint=Muted,modifier=Modifier.size(24.dp))
            Spacer(Modifier.height(8.dp))
            Text("先为此属性添加两个关键帧",color=Ink,fontSize=14.sp)
            Text("再移动到两帧之间，调整运动节奏",color=Muted,fontSize=12.sp)
        }
        }
        Row(Modifier.fillMaxWidth().height(72.dp).horizontalScroll(rememberScrollState()).padding(horizontal=8.dp),horizontalArrangement=Arrangement.spacedBy(6.dp)) {
            easeNames.forEach{(key,label)->CurvePreset(key,label,definition.optJSONObject("curve")==null&&definition.optString("ease")==key,segment!=null&&vm.editable()) {
                draft=null;vm.ease(key)
            }}
            listOf("quadratic" to "二次贝塞尔","cubic" to "三次贝塞尔","elastic" to "弹性").forEach{(kind,label)->
                CurvePreset(kind,label,definition.optJSONObject("curve")?.optJSONObject("shape")?.optString("kind")==kind,segment!=null&&vm.editable(),Modifier.testTag("curve-kind-"+kind)) {
                    draft=null;vm.setCurve(defaultCurve(kind,view))
                }
            }
        }
    }
    if(parameters)CurveParameters(definition,onDismiss={parameters=false}){vm.setCurve(it);parameters=false}
}

@Composable private fun CurvePreset(kind:String,label:String,selected:Boolean,enabled:Boolean,modifier:Modifier=Modifier,onClick:()->Unit) {
    Surface(onClick=onClick,enabled=enabled,color=if(selected)Accent.copy(alpha=.12f)else Background,shape=RoundedCornerShape(10.dp),
        border=if(selected)BorderStroke(1.dp,Accent.copy(alpha=.6f))else null,
        modifier=modifier.width(76.dp).height(64.dp).semantics{this.selected=selected}) {
        Column(Modifier.padding(8.dp),horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(4.dp)) {
            Canvas(Modifier.width(40.dp).height(24.dp)) {
                val path=Path()
                for(i in 0..32) {
                    val t=i/32f
                    val y=when(kind) {
                        "in","quadratic"->t*t
                        "out"->1-(1-t)*(1-t)
                        "in_out","cubic"->t*t*(3-2*t)
                        "hold"->if(i==32)1f else 0f
                        "elastic"->(1-exp(-6f*t)*cos(5f*PI.toFloat()*t)).coerceIn(0f,1.3f)/1.3f
                        else->t
                    }
                    if(i==0)path.moveTo(0f,size.height)else path.lineTo(t*size.width,(1-y)*size.height)
                }
                drawPath(path,if(selected)Accent else Muted.copy(alpha=if(enabled).8f else .3f),style=Stroke(1.5.dp.toPx()))
            }
            Text(label,color=if(selected)Accent else Muted,fontSize=11.sp,lineHeight=14.sp,maxLines=1)
        }
    }
}

@Composable private fun CurveParameters(initial:JSONObject,onDismiss:()->Unit,onApply:(JSONObject)->Unit) {
    var space by remember{mutableStateOf(initial.optJSONObject("curve")?.optString("space","progress")?:"progress")}
    var kind by remember{mutableStateOf(initial.optJSONObject("curve")?.optJSONObject("shape")?.optString("kind")?:"cubic")}
    var source by remember{mutableStateOf(if(initial.has("curve"))initial.toString()else defaultCurve(kind,space).toString())}
    var error by remember{mutableStateOf<String?>(null)}
    fun reset(newKind:String,newSpace:String){if(newKind==kind&&newSpace==space)return;kind=newKind;space=newSpace;source=defaultCurve(kind,space).toString();error=null}
    val shape=JSONObject(source).getJSONObject("curve").getJSONObject("shape")
    val fields=when(kind) {
        "quadratic"->listOf("control.x" to "控制点 X","control.y" to "控制点 Y")
        "cubic"->listOf("control1.x" to "控制点 1 X","control1.y" to "控制点 1 Y","control2.x" to "控制点 2 X","control2.y" to "控制点 2 Y")
        else->listOf("oscillations" to "振荡次数","damping" to "衰减")
    }+if(space=="velocity"&&kind!="elastic")listOf("start" to "起始速度","end" to "结束速度")else emptyList()
    val values=remember(source){fields.associate{(key,_)->key to mutableStateOf((if(key.contains('.'))shape.getJSONArray(key.substringBefore('.')).getDouble(if(key.endsWith('x'))0 else 1)else shape.optDouble(key,if(key=="end")1.0 else 0.0)).toString())}}
    AlertDialog(onDismissRequest=onDismiss,title={Text("曲线参数")},text={Column(Modifier.verticalScroll(rememberScrollState())) {
        Row(Modifier.horizontalScroll(rememberScrollState())) {
            listOf("progress" to "按进度定义","velocity" to "按速度定义").forEach{(key,label)->TextButton(onClick={reset(kind,key)},modifier=Modifier.height(48.dp)){Text(label,color=if(space==key)Accent else Muted)}}
        }
        Text(if(space=="velocity")"速度曲线按面积归一化，积分后到达终点。"else"控制点允许超出 0–1，产生超越与回弹。",fontSize=12.sp,color=Muted)
        Row(Modifier.horizontalScroll(rememberScrollState())) {
            listOf("quadratic" to "二次","cubic" to "三次","elastic" to "弹性").forEach{(key,label)->TextButton(onClick={reset(key,space)},modifier=Modifier.height(48.dp)){Text(label,color=if(kind==key)Accent else Muted)}}
        }
        fields.forEach{(key,label)->Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
            OutlinedTextField(value=values.getValue(key).value,onValueChange={values.getValue(key).value=it;error=null},label={Text(label)},singleLine=true,
                keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Decimal),modifier=Modifier.weight(1f).padding(vertical=3.dp).testTag("curve-param-"+key))
            if(key.endsWith(".y")||key=="start"||key=="end")TextButton(onClick={val text=values.getValue(key).value;values.getValue(key).value=if(text.startsWith("-"))text.drop(1)else"-"+text;error=null},modifier=Modifier.size(48.dp)){Text("±")}
        }}
        if(error!=null)Text(error!!,color=MaterialTheme.colorScheme.error,fontSize=12.sp)
    }},confirmButton={TextButton(onClick={
        val edited=JSONObject(source);val target=edited.getJSONObject("curve").getJSONObject("shape")
        var valid=true
        fields.forEach{(key,_)->val value=values.getValue(key).value.toDoubleOrNull();if(value==null||!value.isFinite())valid=false
            else if(key.contains('.'))target.getJSONArray(key.substringBefore('.')).put(if(key.endsWith('x'))0 else 1,value)else target.put(key,value)}
        if(valid&&graphData(edited.toString())!=null)onApply(edited)else error="请检查参数：X 为 0–1；Y 为 −8–8；速度面积不能为零。"
    }){Text("应用")}},dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}
