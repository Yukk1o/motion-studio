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
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

/** Local control-point draft; applying produces one reversible native edit. */
@Composable internal fun EffectCurveObject(vm:EditorViewModel,objectId:Long,instance:Long,param:String,saved:JSONObject,enabled:Boolean) {
    val track=saved.getJSONObject("curve")
    val local=vm.frame-(vm.timelineLayer(objectId)?.optInt("offset_frame")?:0)
    val keys=track.optJSONArray("keys").objects()
    val value=(vm.sampleValueFor(objectId,"effect:$instance:$param") as? JSONObject)?:keys.lastOrNull{it.getDouble("frame")<=local}?.getJSONObject("value")?:keys.firstOrNull()?.getJSONObject("value")?:track.getJSONObject("value")
    var draft by remember(objectId,instance,param,vm.frame,value.toString()){mutableStateOf(value.toString())}
    var channel by remember{mutableIntStateOf(0)}
    var chosen by remember(channel){mutableIntStateOf(-1)}
    val changed=draft!=value.toString()
    val data=JSONObject(draft)
    val points=data.getJSONArray("channels").getJSONArray(channel)
    val liveDraft by rememberUpdatedState(draft)
    Column {
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            listOf("RGB","R","G","B","Alpha").forEachIndexed{i,label->TextButton(onClick={channel=i},modifier=Modifier.height(48.dp)){Text(label,color=if(channel==i)Accent else Muted)}}
        }
        Canvas(Modifier.fillMaxWidth().height(180.dp).padding(12.dp).testTag("effect-color-curve").pointerInput(channel,enabled) {
            if(!enabled)return@pointerInput
            awaitEachGesture {
                val down=awaitFirstDown()
                val before=liveDraft
                val json=JSONObject(before)
                val curve=json.getJSONArray("channels").getJSONArray(channel)
                fun point(i:Int)=curve.getJSONArray(i).let{Offset(it.getDouble(0).toFloat()*size.width,(1-it.getDouble(1).toFloat())*size.height)}
                var index=(0 until curve.length()).minByOrNull{(point(it)-down.position).getDistance()}?:0
                if((point(index)-down.position).getDistance()>24.dp.toPx()) {
                    if(curve.length()>=64)return@awaitEachGesture
                    val x=(down.position.x/size.width).toDouble().coerceIn(.0001,.9999)
                    index=(0 until curve.length()).first{curve.getJSONArray(it).getDouble(0)>x}
                    val list=(0 until curve.length()).map{curve.getJSONArray(it)}.toMutableList()
                    list.add(index,JSONArray(listOf(x,(1-down.position.y/size.height).toDouble().coerceIn(0.0,1.0))))
                    json.getJSONArray("channels").put(channel,JSONArray(list));json.remove("sampled_lut");draft=json.toString()
                }
                chosen=index;down.consume()
                val origin=JSONObject(draft).getJSONArray("channels").getJSONArray(channel).getJSONArray(index)
                val originX=origin.getDouble(0);val originY=origin.getDouble(1)
                var movement=Offset.Zero
                val initial=draft
                val completed=drag(down.id){change->
                    movement+=change.position- change.previousPosition;change.consume()
                    val updated=JSONObject(initial);val c=updated.getJSONArray("channels").getJSONArray(channel)
                    val x=when(index){0->0.0;c.length()-1->1.0;else->(originX+movement.x/size.width).coerceIn(c.getJSONArray(index-1).getDouble(0)+.00001,c.getJSONArray(index+1).getDouble(0)-.00001)}
                    c.put(index,JSONArray(listOf(x,(originY-movement.y/size.height).coerceIn(0.0,1.0))))
                    updated.remove("sampled_lut");draft=updated.toString()
                }
                if(!completed){draft=before;chosen=-1}
            }
        }) {
            repeat(5){i->drawLine(Muted.copy(alpha=.18f),Offset(0f,size.height*i/4),Offset(size.width,size.height*i/4),1f);drawLine(Muted.copy(alpha=.18f),Offset(size.width*i/4,0f),Offset(size.width*i/4,size.height),1f)}
            val path=Path()
            for(i in 0 until points.length()){val p=points.getJSONArray(i);val x=p.getDouble(0).toFloat()*size.width;val y=(1-p.getDouble(1).toFloat())*size.height;if(i==0)path.moveTo(x,y)else path.lineTo(x,y)}
            drawPath(path,Accent,style=Stroke(2.dp.toPx()))
            data.optJSONArray("sampled_lut")?.let{lut->
                val sampled=Path()
                for(i in 0 until lut.length()){val row=lut.getJSONArray(i);val component=if(channel==4)3 else (channel-1).coerceAtLeast(0);val x=i.toFloat()*size.width/(lut.length()-1);val y=(1-row.getDouble(component).toFloat())*size.height;if(i==0)sampled.moveTo(x,y)else sampled.lineTo(x,y)}
                drawPath(sampled,Ink.copy(alpha=.7f),style=Stroke(1.dp.toPx()))
            }
            for(i in 0 until points.length()){val p=points.getJSONArray(i);drawCircle(if(i==chosen)Ink else Accent,3.dp.toPx(),Offset(p.getDouble(0).toFloat()*size.width,(1-p.getDouble(1).toFloat())*size.height))}
        }
        Text("轻触添加点，拖动调整。应用后保存当前帧。",color=Muted,fontSize=12.sp)
        Row(Modifier.fillMaxWidth().testTag("effect-color-curve-actions").horizontalScroll(rememberScrollState())) {
            TextButton(onClick={val j=JSONObject(draft);val c=j.getJSONArray("channels").getJSONArray(channel);val a=JSONArray();for(i in 0 until c.length())if(i!=chosen)a.put(c.getJSONArray(i));j.getJSONArray("channels").put(channel,a);j.remove("sampled_lut");draft=j.toString();chosen=-1},enabled=enabled&&chosen>0&&chosen<points.length()-1,modifier=Modifier.height(48.dp)){Text("删除点")}
            TextButton(onClick={val j=JSONObject(draft);j.getJSONArray("channels").put(channel,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,1))));j.remove("sampled_lut");draft=j.toString()},enabled=enabled,modifier=Modifier.height(48.dp)){Text("重置通道")}
            TextButton(onClick={vm.effectAction(objectId,instance,"set_curve_object",JSONObject().put("param",param).put("frame",floor(vm.frame).toInt()).put("value",JSONObject(draft)))},enabled=enabled&&changed,modifier=Modifier.height(48.dp).testTag("effect-color-curve-apply")){Text("应用")}
        }
    }
}
