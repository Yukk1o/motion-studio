package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

private val curveNames=listOf("RGB","R","G","B","Alpha")
private val curveColors=listOf(Color(0xFFD1D8E2),Color(0xFFFF656D),Color(0xFF69EB88),Color(0xFF65A6FF),Color(0xFFFFCF74))
private fun JSONArray.arrays()=(0 until length()).map{getJSONArray(it)}
internal fun colorCurveGraph(value:JSONObject)=nativeData(NativeBridge.colorCurveGraph(value.toString()))
private fun editableCurve(value:JSONObject)=colorCurveGraph(value).getJSONObject("editableValue")
private fun editCurve(draft:String,channel:Int,block:(JSONObject,JSONArray)->Unit):String {
    val result=JSONObject(draft);result.optJSONArray("channel_luts")?.put(channel,JSONObject.NULL);result.remove("sampled_lut")
    block(result,result.getJSONArray("channels").getJSONArray(channel));return result.toString()
}

/** RGB is an overview; independent Alpha controls transparency, never RGB. */
@Composable internal fun EffectCurveObject(vm:EditorViewModel,objectId:Long,instance:Long,param:String,saved:JSONObject,enabled:Boolean) {
    val track=saved.getJSONObject("curve")
    val local=vm.frame-(vm.timelineLayer(objectId)?.optInt("offset_frame")?:0)
    val keys=track.optJSONArray("keys").objects()
    val value=(vm.sampleValueFor(objectId,"effect:$instance:$param") as? JSONObject)
        ?:keys.lastOrNull{it.getDouble("frame")<=local}?.getJSONObject("value")?:keys.firstOrNull()?.getJSONObject("value")?:track.getJSONObject("value")
    var draft by remember(objectId,instance,param){mutableStateOf(editableCurve(value).toString())}
    var tab by remember(objectId,instance,param){mutableIntStateOf(0)}
    var channel by remember(objectId,instance,param){mutableIntStateOf(0)}
    var chosen by remember(channel){mutableIntStateOf(-1)}
    var gesture by remember(objectId,instance,param){mutableStateOf(false)}
    var capturedFrame by remember{mutableIntStateOf(0)}
    val graph=remember(draft){colorCurveGraph(JSONObject(draft))}
    val data=JSONObject(draft);val points=data.getJSONArray("channels").getJSONArray(channel)
    val curves=graph.getJSONArray("channels").objects()
    val liveDraft by rememberUpdatedState(draft);val liveChannel by rememberUpdatedState(channel);val liveTab by rememberUpdatedState(tab)
    LaunchedEffect(value.toString(),vm.frame,gesture){if(!gesture)draft=editableCurve(value).toString()}
    fun begin(){if(!gesture){vm.pause();vm.chooseEffectParam(instance,param);capturedFrame=floor(vm.frame).toInt();vm.beginGesture();gesture=true}}
    fun publish(next:String){draft=next;vm.effectAction(objectId,instance,"set_curve_object",JSONObject().put("param",param).put("frame",capturedFrame).put("value",JSONObject(next)),false)}
    fun finish(){if(gesture)vm.endGesture{gesture=false}}
    fun cancel(before:String){if(gesture)vm.cancelGesture();gesture=false;draft=before;chosen=-1}
    fun discrete(next:String){begin();publish(next);finish()}
    DisposableEffect(objectId,instance,param){onDispose{if(gesture)vm.cancelGesture()}}
    Column {
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).testTag("color-curve-tabs")) {
            curveNames.forEachIndexed{i,label->TextButton(onClick={vm.chooseEffectParam(instance,param);tab=i;channel=i;chosen=-1},enabled=!gesture,
                modifier=Modifier.heightIn(min=48.dp).testTag("color-curve-channel-$i").semantics{selected=tab==i}){Text(if(i==0)"RGB 总览"else label,color=if(tab==i)curveColors[i]else Muted)}}
        }
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            Text("编辑 ${curveNames[channel]}",color=curveColors[channel],fontSize=12.sp,modifier=Modifier.padding(top=16.dp,bottom=16.dp,end=10.dp))
            val mode=data.optJSONArray("interpolation")?.optString(channel,"linear")?:"linear"
            listOf("natural_cubic" to "平滑","linear" to "折线").forEach{(id,label)->TextButton(onClick={
                discrete(editCurve(draft,channel){json,_->json.put("interpolation",(json.optJSONArray("interpolation")?:JSONArray(List(5){"linear"})).put(channel,id))})
            },enabled=enabled&&!gesture,modifier=Modifier.heightIn(min=48.dp).testTag("color-curve-mode-$id").semantics{selected=mode==id}){Text(label,color=if(mode==id)Accent else Muted)}}
        }
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val edge=maxWidth.coerceAtMost(320.dp)
            Canvas(Modifier.width(edge).height(180.dp).padding(8.dp).testTag("effect-color-curve")
                .semantics{contentDescription=if(tab==0)"RGB 总览：RGB、R、G、B、Alpha；编辑 ${curveNames[channel]}"else"${curveNames[channel]} 曲线"}
                .pointerInput(enabled){
                    if(!enabled)return@pointerInput
                    awaitEachGesture {
                        val down=awaitFirstDown();val before=liveDraft;var active=liveChannel
                        if(liveTab==0) {
                            val plot=colorCurveGraph(JSONObject(before)).getJSONArray("channels").objects()
                            val index=((down.position.x/size.width).coerceIn(0f,1f)*255).roundToInt()
                            val candidates=(0..4).map{i->i to abs((1-plot[i].getJSONArray("samples").getJSONArray(index).getDouble(1))*size.height-down.position.y)}
                            val nearest=candidates.minBy{it.second};val current=candidates[active].second
                            if(nearest.second<24.dp.toPx()&&current-nearest.second>2.dp.toPx()){active=nearest.first;channel=active}
                        }
                        val curve=JSONObject(before).getJSONArray("channels").getJSONArray(active)
                        fun point(i:Int)=curve.getJSONArray(i).let{Offset(it.getDouble(0).toFloat()*size.width,(1-it.getDouble(1).toFloat())*size.height)}
                        var index=(0 until curve.length()).minBy{(point(it)-down.position).getDistance()};var initial=before
                        if((point(index)-down.position).getDistance()>24.dp.toPx()) {
                            if(curve.length()>=64)return@awaitEachGesture
                            val x=(down.position.x/size.width).toDouble().coerceIn(.0001,.9999)
                            index=(0 until curve.length()).first{curve.getJSONArray(it).getDouble(0)>x}
                            if(x.toFloat()<=curve.getJSONArray(index-1).getDouble(0).toFloat()||x.toFloat()>=curve.getJSONArray(index).getDouble(0).toFloat())return@awaitEachGesture
                            val list=curve.arrays().toMutableList();list.add(index,JSONArray(listOf(x,(1-down.position.y/size.height).toDouble().coerceIn(0.0,1.0))))
                            initial=editCurve(before,active){json,_->json.getJSONArray("channels").put(active,JSONArray(list))};begin();publish(initial)
                        }
                        chosen=index;down.consume();val origin=JSONObject(initial).getJSONArray("channels").getJSONArray(active).getJSONArray(index);var movement=Offset.Zero
                        val completed=drag(down.id){change->
                            movement+=change.position-change.previousPosition;change.consume();begin()
                            val next=editCurve(initial,active){_,c->
                                val min=if(index==0)0.0 else Math.nextUp(c.getJSONArray(index-1).getDouble(0).toFloat()).toDouble()
                                val max=if(index==c.length()-1)1.0 else Math.nextDown(c.getJSONArray(index+1).getDouble(0).toFloat()).toDouble()
                                val x=when(index){0->0.0;c.length()-1->1.0;else->if(min<=max)(origin.getDouble(0)+movement.x/size.width).coerceIn(min,max)else origin.getDouble(0)}
                                c.put(index,JSONArray(listOf(x,(origin.getDouble(1)-movement.y/size.height).coerceIn(0.0,1.0))))
                            };publish(next)
                        };if(completed)finish()else cancel(before)
                    }
                }) {
                drawRect(Background)
                repeat(5){i->drawLine(Muted.copy(alpha=.18f),Offset(0f,size.height*i/4),Offset(size.width,size.height*i/4),1f);drawLine(Muted.copy(alpha=.18f),Offset(size.width*i/4,0f),Offset(size.width*i/4,size.height),1f)}
                drawLine(Muted.copy(alpha=.4f),Offset(0f,size.height),Offset(size.width,0f),1f)
                val visible=if(tab==0)(0..4).toList()else listOf(channel)
                clipRect {
                    visible.forEach{i->
                        val curve=curves[i];val path=Path()
                        val segments=curve.getJSONArray("segments").arrays()
                        val clipped=segments.any{s->s.arrays().any{p->p.getDouble(1) !in 0.0..1.0}}
                        if(curve.getBoolean("lookup")||clipped)curve.getJSONArray("samples").arrays().forEachIndexed{j,p->val x=p.getDouble(0).toFloat()*size.width;val y=(1-p.getDouble(1).toFloat())*size.height;if(j==0)path.moveTo(x,y)else path.lineTo(x,y)}
                        else curve.getJSONArray("segments").arrays().forEachIndexed{j,s->
                            fun xy(k:Int)=s.getJSONArray(k).let{Offset(it.getDouble(0).toFloat()*size.width,(1-it.getDouble(1).toFloat())*size.height)}
                            val a=xy(0);val b=xy(1);val c=xy(2);val d=xy(3);if(j==0)path.moveTo(a.x,a.y);path.cubicTo(b.x,b.y,c.x,c.y,d.x,d.y)
                        }
                        val overlapping=visible.filter{curves[it].getJSONArray("samples").toString()==curve.getJSONArray("samples").toString()}
                        val dash=if(overlapping.size>1)PathEffect.dashPathEffect(floatArrayOf(7.dp.toPx(),7.dp.toPx()*(overlapping.size-1)),overlapping.indexOf(i)*7.dp.toPx())else null
                        drawPath(path,curveColors[i],style=Stroke(if(i==channel)2.5.dp.toPx()else 1.8.dp.toPx(),pathEffect=dash))
                    }
                }
                for(i in 0 until points.length()){val p=points.getJSONArray(i);val at=Offset(p.getDouble(0).toFloat()*size.width,(1-p.getDouble(1).toFloat())*size.height);drawCircle(Background,6.dp.toPx(),at);drawCircle(if(i==chosen)Ink else curveColors[channel],4.dp.toPx(),at)}
            }
        }
        Text(if(tab==0)"点选彩色曲线切换编辑通道。总曲线调颜色，Alpha 调透明度。"else"轻触添加点，拖动实时预览；一次拖动可一次撤销。",color=Muted,fontSize=12.sp)
        Canvas(Modifier.fillMaxWidth().height(36.dp).padding(vertical=4.dp).testTag("curve-alpha-preview").semantics{contentDescription="颜色和 Alpha 曲线的棋盘格预览"}) {
            checkerboard();val lut=graph.getJSONArray("outputLut")
            for(i in 0 until lut.length()){val c=lut.getJSONArray(i);drawRect(Color(c.getDouble(0).toFloat(),c.getDouble(1).toFloat(),c.getDouble(2).toFloat(),c.getDouble(3).toFloat()),Offset(i*size.width/lut.length(),0f),androidx.compose.ui.geometry.Size(size.width/lut.length()+1,size.height))}
        }
        Row(Modifier.fillMaxWidth().testTag("effect-color-curve-actions").horizontalScroll(rememberScrollState())) {
            TextButton(onClick={discrete(editCurve(draft,channel){json,c->json.getJSONArray("channels").put(channel,JSONArray(c.arrays().filterIndexed{i,_->i!=chosen}))});chosen=-1},enabled=enabled&&!gesture&&chosen>0&&chosen<points.length()-1,modifier=Modifier.heightIn(min=48.dp).testTag("color-curve-delete")){Text("删除点")}
            TextButton(onClick={discrete(editCurve(draft,channel){json,_->json.getJSONArray("channels").put(channel,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,1))))});chosen=-1},enabled=enabled&&!gesture,modifier=Modifier.heightIn(min=48.dp).testTag("color-curve-reset")){Text("重置通道")}
        }
    }
}
