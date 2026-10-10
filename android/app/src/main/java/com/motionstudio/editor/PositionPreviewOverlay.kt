package com.motionstudio.editor

import android.opengl.Matrix
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.min

internal class PositionProjection(matrix:JSONArray,width:Float,height:Float,compositionWidth:Int,compositionHeight:Int) {
    private val m=FloatArray(16){matrix.getDouble(it).toFloat()}
    private val inverse=FloatArray(16)
    private val valid=Matrix.invertM(inverse,0,m,0)
    private val fit=min(width/compositionWidth,height/compositionHeight)
    private val w=compositionWidth*fit;private val h=compositionHeight*fit
    private val left=(width-w)/2;private val top=(height-h)/2
    fun screen(value:JSONArray):Offset? {
        val v=FloatArray(4);Matrix.multiplyMV(v,0,m,0,floatArrayOf(value.optDouble(0).toFloat(),value.optDouble(1).toFloat(),value.optDouble(2).toFloat(),1f),0)
        if(!v[3].isFinite()||v[3]<=.00001f)return null
        return Offset(left+(v[0]/v[3]+1)*w/2,top+(1-v[1]/v[3])*h/2).takeIf{it.x.isFinite()&&it.y.isFinite()}
    }
    fun local(point:Offset,z:Double):JSONArray? {
        if(!valid||w<=0||h<=0)return null
        fun unproject(depth:Float):FloatArray? {
            val v=FloatArray(4);Matrix.multiplyMV(v,0,inverse,0,floatArrayOf(2*(point.x-left)/w-1,1-2*(point.y-top)/h,depth,1f),0)
            if(abs(v[3])<1e-8f)return null
            return FloatArray(3){v[it]/v[3]}.takeIf{it.all(Float::isFinite)}
        }
        val a=unproject(0f)?:return null;val b=unproject(1f)?:return null
        val dz=b[2]-a[2];if(abs(dz)<1e-7f)return null
        val t=(z-a[2])/dz
        val x=a[0]+t*(b[0]-a[0]);val y=a[1]+t*(b[1]-a[1])
        if(!x.isFinite()||!y.isFinite())return null
        return JSONArray(listOf(x,y,z))
    }
}

private data class PositionHit(val frame:Int,val value:JSONArray,val point:Offset,val handle:String?=null,val key:JSONObject?=null)

@Composable internal fun PositionPreviewOverlay(vm:EditorViewModel,nativeActions:PositionEditActions?=null) {
    val target=positionTarget(vm)?:return
    val identity=target.toString()
    var data by remember(vm.root,vm.compositionId,identity){mutableStateOf<JSONObject?>(null)}
    var focused by remember(identity){mutableStateOf<Int?>(null)}
    val actions=nativeActions?:remember(vm,identity){ProjectPositionActions(vm)}
    val liveData by rememberUpdatedState(data)
    val liveProject by rememberUpdatedState(vm.state.project)
    val liveTarget by rememberUpdatedState(target)
    val liveActions by rememberUpdatedState(actions)
    val curveMode by rememberUpdatedState(vm.curvePanelOpen)
    val revision=vm.state.sample?.optLong("revision")?:0
    LaunchedEffect(vm.root,vm.compositionId,identity,revision,vm.frame) {vm.positionGeometry(target){result->data=result}}
    LaunchedEffect(vm.frame){focused=floor(vm.frame).toInt()}
    Canvas(Modifier.fillMaxSize().testTag("position-preview-overlay").semantics{stateDescription=if(data==null)"正在加载位置轨迹"else"位置轨迹就绪"}.pointerInput(identity) {
        awaitEachGesture {
            val down=awaitFirstDown(requireUnconsumed=false)
            val path=liveData?:return@awaitEachGesture
            val p=liveProject?:return@awaitEachGesture
            val projection=PositionProjection(path.getJSONArray("matrix"),size.width.toFloat(),size.height.toFloat(),p.getInt("width"),p.getInt("height"))
            val hits=mutableListOf<PositionHit>()
            path.getJSONArray("keys").objects().forEach{key->
                val value=key.getJSONArray("value");val at=key.getInt("frame")
                projection.screen(value)?.let{hits.add(PositionHit(at,value,it,key=key))}
                if(curveMode&&path.optBoolean("spatial_editable")&&focused==at)listOf("incoming","outgoing").forEach{handle->
                    key.optJSONArray(handle)?.let{v->projection.screen(v)?.let{hits.add(PositionHit(at,v,it,handle,key))}}
                }
            }
            val current=path.getJSONArray("value")
            projection.screen(current)?.let{hits.add(PositionHit(floor(vm.frame).toInt(),current,it))}
            if(!curveMode)hits.removeAll{it.key!=null}
            val hit=hits.filter{(it.point-down.position).getDistance()<=20.dp.toPx()}.minByOrNull{(it.point-down.position).getDistance()}
            if(hit==null) {
                if(!curveMode||!path.optBoolean("spatial_editable")||vm.eyedropperActive)return@awaitEachGesture
                down.consume();var moved=false;var released=false
                do {val event=awaitPointerEvent();val change=event.changes.firstOrNull{it.id==down.id}?:break
                    moved=moved||(change.position-down.position).getDistance()>viewConfiguration.touchSlop
                    change.consume();released=!change.pressed
                }while(!released)
                val at=floor(vm.frame).toInt()
                if(released&&!moved) {
                    if(path.getJSONArray("keys").objects().any{it.getInt("frame")==at})vm.showOperationError("当前帧已有路径点，请选择没有关键帧的时刻再添加。")
                    else if(path.getJSONArray("keys").length()==0)vm.showOperationError("先添加一个位置关键帧，再选择新的时刻添加路径点。")
                    else projection.local(down.position,current.optDouble(2))?.let{point->
                        repeat(3){i->point.put(i,point.optDouble(i).coerceIn(path.optDouble("minimum",-1e7),path.optDouble("maximum",1e7)))}
                        liveActions.begin(liveTarget);liveActions.value(point,listOf(0,1),at);liveActions.finish(true);focused=at
                    }
                }
                return@awaitEachGesture
            }
            if(vm.eyedropperActive||!path.optBoolean("editable"))return@awaitEachGesture
            down.consume()
            if(hit.key!=null&&hit.handle==null)focused=hit.frame
            val start=projection.local(down.position,hit.value.optDouble(2))?:return@awaitEachGesture
            val capturedTarget=JSONObject(liveTarget.toString());val edit=liveActions
            var active=false;var ended=false
            try {
                do {
                    val event=awaitPointerEvent();val change=event.changes.firstOrNull{it.id==down.id}?:break
                    if(change.isConsumed)break
                    val local=projection.local(change.position,hit.value.optDouble(2))
                    if(local!=null&&(active||(change.position-down.position).getDistance()>viewConfiguration.touchSlop)) {
                        if(!active){edit.begin(capturedTarget);active=true}
                        val moved=JSONArray(List(3){i->hit.value.optDouble(i)+local.optDouble(i)-start.optDouble(i)})
                        val minimum=path.optDouble("minimum",-1e7);val maximum=path.optDouble("maximum",1e7)
                        repeat(3){i->moved.put(i,moved.optDouble(i).coerceIn(minimum,maximum))}
                        if(hit.handle!=null&&hit.key!=null) {
                            val key=hit.key;val spatial=key.optJSONObject("spatial")?.let{JSONObject(it.toString())}?:JSONObject()
                            listOf("incoming","outgoing").filter{key.optJSONArray(it)==null}.forEach{spatial.remove(it)}
                            val tangent=JSONArray(List(3){i->moved.optDouble(i)-key.getJSONArray("value").optDouble(i)})
                            if(capturedTarget.optString("kind")=="effect")tangent.put(0)
                            spatial.put(hit.handle,tangent);edit.spatial(hit.frame,spatial)
                        } else edit.value(moved,listOf(0,1),hit.frame)
                        change.consume()
                    }
                    ended=!change.pressed
                }while(!ended)
            }finally{if(active)edit.finish(ended)}
            if(!active&&ended&&hit.key!=null&&hit.handle==null)vm.seek(hit.frame.toDouble(),nativeActions!=null)
        }
    }) {
        val path=data?:return@Canvas;val p=vm.state.project?:return@Canvas
        val projection=PositionProjection(path.getJSONArray("matrix"),size.width,size.height,p.getInt("width"),p.getInt("height"))
        val line=Path();var started=false
        path.getJSONArray("samples").objects().forEach{sample->
            val point=projection.screen(sample.getJSONArray("value"))
            if(point==null)started=false else if(started)line.lineTo(point.x,point.y)else{line.moveTo(point.x,point.y);started=true}
        }
        drawPath(line,Accent.copy(alpha=.6f),style=Stroke(1.3.dp.toPx()))
        path.getJSONArray("keys").objects().forEach{key->
            val value=key.getJSONArray("value");val center=projection.screen(value)?:return@forEach
            drawCircle(Accent,if(focused==key.getInt("frame"))4.dp.toPx()else 3.dp.toPx(),center)
            if(vm.curvePanelOpen&&path.optBoolean("spatial_editable")&&focused==key.getInt("frame"))listOf("incoming","outgoing").forEach{handle->
                key.optJSONArray(handle)?.let{v->projection.screen(v)?.let{point->
                    drawLine(Muted.copy(alpha=.7f),center,point,1.dp.toPx())
                    drawCircle(Background,5.dp.toPx(),point);drawCircle(Ink,5.dp.toPx(),point,style=Stroke(1.5.dp.toPx()))
                }}
            }
        }
        projection.screen(path.getJSONArray("value"))?.let{point->
            drawCircle(Background,8.dp.toPx(),point);drawCircle(Ink,7.dp.toPx(),point,style=Stroke(1.5.dp.toPx()));drawCircle(Accent,4.dp.toPx(),point)
        }
    }
}
