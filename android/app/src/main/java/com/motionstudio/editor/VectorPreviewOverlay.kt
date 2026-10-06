package com.motionstudio.editor

import android.opengl.Matrix
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

/** The render matrix includes parenting, 3D and camera transforms. */
internal class VectorProjection(matrix:JSONArray,width:Float,height:Float,compositionWidth:Int,compositionHeight:Int) {
    private val m=FloatArray(16){matrix.getDouble(it).toFloat()}
    private val inverse=FloatArray(16)
    private val valid=Matrix.invertM(inverse,0,m,0)
    private val fit=min(width/compositionWidth,height/compositionHeight)
    private val w=compositionWidth*fit;private val h=compositionHeight*fit
    private val left=(width-w)/2;private val top=(height-h)/2
    fun screen(x:Float,y:Float):Offset? {
        val v=FloatArray(4);Matrix.multiplyMV(v,0,m,0,floatArrayOf(x,-y,0f,1f),0)
        if(!v[3].isFinite()||v[3]<=.00001f)return null
        return Offset(left+(v[0]/v[3]+1)*w/2,top+(1-v[1]/v[3])*h/2).takeIf{it.x.isFinite()&&it.y.isFinite()}
    }
    fun local(point:Offset):Offset? {
        if(!valid||w<=0||h<=0)return null
        fun unproject(z:Float):FloatArray? {
            val v=FloatArray(4);Matrix.multiplyMV(v,0,inverse,0,floatArrayOf(2*(point.x-left)/w-1,1-2*(point.y-top)/h,z,1f),0)
            if(abs(v[3])<.00001f)return null
            return FloatArray(3){v[it]/v[3]}.takeIf{it.all{value->value.isFinite()}}
        }
        val a=unproject(-1f)?:return null;val b=unproject(1f)?:return null
        val dz=b[2]-a[2];if(abs(dz)<.00001f)return null
        val t=-a[2]/dz
        return Offset(a[0]+t*(b[0]-a[0]),-(a[1]+t*(b[1]-a[1]))).takeIf{it.x.isFinite()&&it.y.isFinite()&&abs(it.x)<=32768&&abs(it.y)<=32768}
    }
}

private data class VectorHit(val path:Long,val node:Long,val axis:Int,val geometry:JSONArray,val point:Offset)

@Composable internal fun VectorPreviewOverlay(vm:EditorViewModel) {
    val sample by rememberUpdatedState(vm.vectorSample())
    val project by rememberUpdatedState(vm.state.project)
    var draft by remember(vm.selected){mutableStateOf<VectorHit?>(null)}
    Canvas(Modifier.fillMaxSize().testTag("vector-preview-overlay").pointerInput(vm.selected) {
        awaitEachGesture {
            val down=awaitFirstDown()
            val snapshot=sample?:return@awaitEachGesture
            val p=project?:return@awaitEachGesture
            val projection=VectorProjection(snapshot.getJSONArray("mvp"),size.width.toFloat(),size.height.toFloat(),p.getInt("width"),p.getInt("height"))
            val localStart=projection.local(down.position)?:return@awaitEachGesture
            val hits=mutableListOf<VectorHit>()
            snapshot.optJSONArray("paths").objects().forEach{path->path.optJSONArray("nodes").objects().forEach{node->
                val g=node.getJSONArray("geometry");val x=g.getDouble(0).toFloat();val y=g.getDouble(1).toFloat()
                projection.screen(x,y)?.let{hits.add(VectorHit(path.getLong("id"),node.getLong("id"),0,JSONArray(g.toString()),it))}
                if(path.getLong("id")==vm.vectorPathId&&node.getLong("id")==vm.vectorNodeId)listOf(2,4).forEach{axis->
                    if(hypot(g.getDouble(axis),g.getDouble(axis+1))>.001)projection.screen(x+g.getDouble(axis).toFloat(),y+g.getDouble(axis+1).toFloat())?.let{hits.add(VectorHit(path.getLong("id"),node.getLong("id"),axis,JSONArray(g.toString()),it))}
                }
            }}
            val hit=hits.filter{(it.point-down.position).getDistance()<=22.dp.toPx()}.minByOrNull{(it.point-down.position).getDistance()}
            if(!vm.editable())return@awaitEachGesture
            if(hit==null&&!vm.vectorDrawMode)return@awaitEachGesture
            if(hit!=null){vm.vectorPathId=hit.path;vm.vectorNodeId=hit.node;vm.selectVectorTrack("vector:node:${hit.path}:${hit.node}")}
            down.consume()
            var target=hit;var active=false;var ended=false
            val at=floor(vm.frame).toInt();val handleMode=vm.vectorHandleMode
            fun createNode():VectorHit? {
                val paths=vm.vectorData()?.getJSONObject("source")?.optJSONArray("paths").objects()
                if(paths.sumOf{it.getJSONArray("nodes").length()}>=2048){vm.showOperationError("节点数量已达到上限");return null}
                val path=paths.firstOrNull{it.getLong("id")==vm.vectorPathId}?:return null
                val node=(paths.flatMap{it.getJSONArray("nodes").objects()}.maxOfOrNull{it.getLong("id")}?:0)+1
                val g=JSONArray(listOf(localStart.x,localStart.y,0,0,0,0))
                vm.beginGesture();active=true
                vm.updateVectorPaths(false){it.first{row->row.getLong("id")==vm.vectorPathId}.getJSONArray("nodes").put(JSONObject().put("id",node).put("geometry",JSONObject().put("value",g).put("keys",JSONArray())))}
                vm.vectorNodeId=node;vm.selectVectorTrack("vector:node:${vm.vectorPathId}:$node")
                return VectorHit(vm.vectorPathId,node,4,g,down.position)
            }
            try {
                while(true) {
                    val event=awaitPointerEvent()
                    if(event.changes.size!=1||event.changes.any{it.isConsumed})break
                    val change=event.changes.first()
                    if(!change.pressed){ended=true;if(target==null)target=createNode();break}
                    if(!active&&(change.position-down.position).getDistance()>viewConfiguration.touchSlop) {
                        if(target==null)target=createNode()else {vm.beginGesture();active=true}
                    }
                    if(active)target?.let{captured->
                        val point=projection.local(change.position)?:return@let
                        val g=JSONArray(captured.geometry.toString())
                        if(captured.axis==0){g.put(0,g.getDouble(0)+point.x-localStart.x);g.put(1,g.getDouble(1)+point.y-localStart.y)}
                        else {
                            val dx=(point.x-g.getDouble(0)).coerceIn(-32768.0,32768.0);val dy=(point.y-g.getDouble(1)).coerceIn(-32768.0,32768.0);g.put(captured.axis,dx);g.put(captured.axis+1,dy)
                            val other=if(captured.axis==2)4 else 2
                            if(hit==null||handleMode=="symmetric"){g.put(other,-dx);g.put(other+1,-dy)}
                            else if(handleMode=="smooth"){val length=hypot(g.getDouble(other),g.getDouble(other+1));val moving=hypot(dx,dy);if(moving>.0001){g.put(other,-dx*length/moving);g.put(other+1,-dy*length/moving)}}
                        }
                        draft=captured.copy(geometry=g);vm.setVectorNode(captured.path,captured.node,g,at)
                    }
                    change.consume()
                }
            }finally {
                if(active){if(ended)vm.endGesture{draft=null}else{vm.cancelGesture();draft=null}}
            }
        }
    }) {
        val snapshot=sample?:return@Canvas;val p=project?:return@Canvas
        val projection=VectorProjection(snapshot.getJSONArray("mvp"),size.width,size.height,p.getInt("width"),p.getInt("height"))
        snapshot.optJSONArray("paths").objects().forEach{path->path.optJSONArray("nodes").objects().forEach{node->
            val selected=path.getLong("id")==vm.vectorPathId&&node.getLong("id")==vm.vectorNodeId
            val g=if(draft?.path==path.getLong("id")&&draft?.node==node.getLong("id"))draft!!.geometry else node.getJSONArray("geometry")
            val x=g.getDouble(0).toFloat();val y=g.getDouble(1).toFloat();val center=projection.screen(x,y)?:return@forEach
            if(selected)listOf(2,4).forEach{axis->if(hypot(g.getDouble(axis),g.getDouble(axis+1))>.001)projection.screen(x+g.getDouble(axis).toFloat(),y+g.getDouble(axis+1).toFloat())?.let{point->drawLine(Accent.copy(alpha=.6f),center,point,1.dp.toPx());drawCircle(Ink,3.dp.toPx(),point)}}
            drawCircle(if(selected)Accent else Ink,if(selected)4.dp.toPx()else 3.dp.toPx(),center)
        }}
    }
}
