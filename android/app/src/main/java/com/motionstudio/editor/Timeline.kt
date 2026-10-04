package com.motionstudio.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import java.util.Locale
import kotlin.math.*

private data class TrackRow(val id:Long,val name:String,val color:Color,val visible:Boolean,val locked:Boolean,
    val property:String,val keys:List<Int>,val allKeys:Set<Int>)
private data class KeyTarget(val objectId:Long,val property:String,val frame:Int)

@Composable internal fun Timeline(vm:EditorViewModel,modifier:Modifier) {
    val density=LocalDensity.current.density
    val project=vm.state.project
    val rows=remember(project,vm.selected,vm.property){buildList {
        if(project!=null) {
            fun keys(track:JSONObject?)=track?.optJSONArray("keys")?.let{a->(0 until a.length()).map{a.getJSONObject(it).getInt("frame")}}?:emptyList()
            fun allKeys(transform:JSONObject)=transform.keys().asSequence().flatMap{keys(transform.optJSONObject(it)).asSequence()}.toSet()
            val camera=project.getJSONObject("camera")
            val cameraKey=if(vm.selected==0L)vm.property else if(camera.getString("mode")=="orbit")"radius" else "position"
            add(TrackRow(0,"摄影机 1",Color(0xFFE5C17E),true,false,cameraKey,keys(camera.optJSONObject(cameraKey)),allKeys(camera)))
            val layers=project.getJSONArray("layers")
            for(i in layers.length()-1 downTo 0) {
                val l=layers.getJSONObject(i);val key=if(vm.selected==l.getLong("id"))vm.property else "position"
                val t=l.getJSONObject("transform")
                add(TrackRow(l.getLong("id"),l.getString("name"),listOf(Color(0xFF6EADE8),Color(0xFFAD9DE0),Color(0xFF67BFAF))[i%3],
                    l.getBoolean("visible"),l.getBoolean("locked"),key,keys(t.optJSONObject(key)),allKeys(t)))
            }
        }
    }}
    val currentRows by rememberUpdatedState(rows)
    var vertical by remember{mutableFloatStateOf(0f)}
    var movedKey by remember{mutableStateOf<KeyTarget?>(null)}
    var editKey by remember{mutableStateOf<KeyTarget?>(null)}
    var contextRow by remember{mutableStateOf<TrackRow?>(null)}
    var jumpDialog by remember{mutableStateOf(false)}
    var hoveredRow by remember{mutableStateOf<Int?>(null)}
    Canvas(modifier.testTag("timeline").pointerInput(Unit) {
        var lastTapId=-1L;var lastTapTime=0L
        awaitEachGesture {
            val down=awaitFirstDown()
            val values=currentRows
            val rowHeight=52*density;val head=44*density
            val startFrame=vm.frame;val startScale=vm.timelineScale*density;val startScroll=vertical
            val rowIndex=floor((down.position.y-head+vertical)/rowHeight).toInt()
            val row=if(down.position.y>=head)values.getOrNull(rowIndex)else null
            val rulerRow=values.firstOrNull{it.id==vm.selected}
            val keyRow=if(down.position.y<16*density)rulerRow else if(row!=null&&down.position.y>=head+rowIndex*rowHeight-vertical+32*density)row else null
            val hitKey=keyRow?.keys?.minByOrNull{abs(size.width/2f+(it-startFrame)*startScale-down.position.x)}
                ?.takeIf{abs(size.width/2f+(it-startFrame)*startScale-down.position.x)<=18*density}
            var total=Offset.Zero;var mode="";var target=hitKey
            do {
                val event=awaitPointerEvent()
                val pan=event.calculatePan();total+=pan
                val elapsed=event.changes.first().uptimeMillis-down.uptimeMillis
                if(event.changes.count{it.pressed}>1) {
                    mode="zoom"
                    val center=event.calculateCentroid(useCurrent=true)
                    val old=vm.timelineScale*density
                    val anchor=vm.frame+(center.x-size.width/2f)/old
                    val next=(vm.timelineScale*event.calculateZoom()).coerceIn(.4f,12f)
                    vm.timelineScale=next
                    vm.seek((anchor-(center.x-size.width/2f)/(next*density)).roundToInt().toDouble())
                } else if(mode.isEmpty()&&total.getDistance()>viewConfiguration.touchSlop) {
                    mode=when {
                        hitKey!=null->"key"
                        row?.id!=null&&row.id!=0L&&elapsed>=viewConfiguration.longPressTimeoutMillis&&abs(total.y)>abs(total.x)->"reorder"
                        abs(total.x)>=abs(total.y)->"scrub"
                        else->"scroll"
                    }
                    if(mode=="key"&&keyRow!=null)vm.select(keyRow.id,false)
                }
                when(mode) {
                    "key"->if(keyRow!=null&&hitKey!=null) {
                        target=(hitKey+total.x/startScale).roundToInt().coerceIn(0,(vm.state.project?.optInt("frames")?:180)-1)
                        movedKey=KeyTarget(keyRow.id,keyRow.property,target)
                    }
                    "scrub"->vm.seek((startFrame-total.x/startScale).roundToInt().toDouble())
                    "scroll"->vertical=(startScroll-total.y).coerceIn(0f,max(0f,values.size*rowHeight-size.height+head))
                    "reorder"->hoveredRow=floor((down.position.y+total.y-head+vertical)/rowHeight).toInt().coerceIn(1,values.lastIndex)
                }
                if(mode.isNotEmpty())event.changes.forEach{it.consume()}
            } while(event.changes.any{it.pressed})
            if(mode=="key"&&keyRow!=null&&hitKey!=null&&target!=hitKey) {
                vm.moveKeyFor(keyRow.id,keyRow.property,hitKey,target!!);vm.seek(target!!.toDouble())
            } else if(mode=="reorder"&&row!=null&&hoveredRow!=null) {
                vm.select(row.id,false);vm.reorderTo(row.id,values.size-1-hoveredRow!!)
            } else if(mode.isEmpty()) {
                val duration=down.uptimeMillis.let{start->currentEvent.changes.first().uptimeMillis-start}
                if(hitKey!=null&&keyRow!=null) {
                    vm.select(keyRow.id,false);vm.seek(hitKey.toDouble())
                    if(duration>=viewConfiguration.longPressTimeoutMillis)editKey=KeyTarget(keyRow.id,keyRow.property,hitKey)
                } else if(row!=null) {
                    if(down.position.x<48*density&&row.id!=0L)vm.flags(row.id,!row.visible,row.locked)
                    else {
                        val doubleTap=lastTapId==row.id&&down.uptimeMillis-lastTapTime<=viewConfiguration.doubleTapTimeoutMillis
                        vm.select(row.id,doubleTap)
                        if(duration>=viewConfiguration.longPressTimeoutMillis)contextRow=row
                        lastTapId=row.id;lastTapTime=down.uptimeMillis
                    }
                } else if(down.position.y<head) {
                    if(duration>=viewConfiguration.longPressTimeoutMillis)jumpDialog=true
                    else vm.seek((startFrame+(down.position.x-size.width/2f)/startScale).roundToInt().toDouble())
                }
            }
            movedKey=null;hoveredRow=null
        }
    }) {
        val rowHeight=52*density;val head=44*density;val center=size.width/2f;val scale=vm.timelineScale*density
        val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply{color=android.graphics.Color.LTGRAY;textSize=11*density}
        val startFrame=max(0,(vm.frame-center/scale).toInt());val endFrame=min(project?.optInt("frames")?:180,ceil(vm.frame+center/scale).toInt())
        for(f in startFrame..endFrame)if(f%5==0) {
            val x=center+(f-vm.frame).toFloat()*scale
            drawLine(Muted.copy(alpha=.35f),Offset(x,0f),Offset(x,if(f%30==0)13*density else 7*density),density)
        }
        fun diamond(frame:Int,y:Float,active:Boolean) {
            val x=center+(frame-vm.frame).toFloat()*scale;val r=if(active)6*density else 4*density
            val path=Path().apply{moveTo(x,y-r);lineTo(x+r,y);lineTo(x,y+r);lineTo(x-r,y);close()}
            drawPath(path,if(active)Accent else Muted.copy(alpha=.28f))
            if(active)drawPath(path,Ink,style=Stroke(density))
        }
        rows.firstOrNull{it.id==vm.selected}?.keys?.forEach{diamond(it,9*density,true)}
        movedKey?.let{diamond(it.frame,9*density,true)}
        val fps=project?.optInt("fps")?:30;val current=floor(vm.frame).toInt()
        val time=String.format(Locale.US,"%02d:%02d:%02d",current/fps/60,current/fps%60,current%fps)
        drawRoundRect(Panel,Offset(center-48*density,17*density),Size(96*density,22*density),androidx.compose.ui.geometry.CornerRadius(4*density))
        paint.textAlign=android.graphics.Paint.Align.CENTER
        drawContext.canvas.nativeCanvas.drawText(time,center,32*density,paint)
        clipRect(top=head) {
            rows.forEachIndexed{index,row->
                val y=head+index*rowHeight-vertical
                if(y+rowHeight<head||y>size.height)return@forEachIndexed
                val x=center-vm.frame.toFloat()*scale;val length=(project?.optInt("frames")?:180)*scale
                val left=max(49*density,x);val width=max(0f,min(size.width-12*density,x+length)-left)
                if(row.id==vm.selected) {
                    drawRect(Accent.copy(alpha=.08f),Offset(0f,y),Size(size.width,rowHeight))
                    drawRoundRect(Accent,Offset(left-2*density,y+5*density),Size(width+4*density,34*density),androidx.compose.ui.geometry.CornerRadius(5*density))
                }
                drawRoundRect(row.color.copy(alpha=if(row.visible)1f else .3f),Offset(left,y+7*density),Size(width,30*density),androidx.compose.ui.geometry.CornerRadius(4*density))
                paint.color=android.graphics.Color.rgb(23,33,41);paint.textAlign=android.graphics.Paint.Align.LEFT;paint.textSize=12*density
                paint.typeface=if(row.id==vm.selected)android.graphics.Typeface.DEFAULT_BOLD else android.graphics.Typeface.DEFAULT
                drawContext.canvas.nativeCanvas.drawText(row.name,left+10*density,y+27*density,paint)
                val eye=Path().apply{moveTo(12*density,y+25*density);quadraticTo(23*density,y+10*density,34*density,y+25*density);quadraticTo(23*density,y+40*density,12*density,y+25*density)}
                drawPath(eye,if(row.visible)Ink else Muted,style=Stroke(1.3f*density))
                if(row.visible)drawCircle(Ink,3*density,Offset(23*density,y+25*density))
                row.allKeys.forEach{diamond(it,y+44*density,row.id==vm.selected&&it in row.keys)}
                if(row.locked)drawRect(Muted,Offset(size.width-19*density,y+16*density),Size(7*density,8*density))
            }
            hoveredRow?.let{index->drawLine(Accent,Offset(48*density,head+index*rowHeight-vertical),Offset(size.width,head+index*rowHeight-vertical),2*density)}
        }
        drawLine(Ink.copy(alpha=.65f),Offset(center,head),Offset(center,size.height),density)
    }
    editKey?.let{key->InputKeyDialog(vm,key){editKey=null}}
    if(jumpDialog)InputDialog("跳到帧",floor(vm.frame).toInt().toString(),onDismiss={jumpDialog=false}){it.toIntOrNull()?.let{frame->vm.seek(frame.toDouble())};jumpDialog=false}
    contextRow?.let{row->AlertDialog(onDismissRequest={contextRow=null},title={Text(row.name)},text={Column {
        TextButton(onClick={vm.select(row.id);contextRow=null}){Text("移动和变换")}
        if(row.id!=0L) {
            TextButton(onClick={vm.duplicate();contextRow=null}){Text("复制图层")}
            TextButton(onClick={vm.flags(row.id,row.visible,!row.locked);contextRow=null}){Text(if(row.locked)"解锁图层" else "锁定图层")}
            TextButton(onClick={vm.deleteLayer();contextRow=null}){Text("删除图层")}
        }
    }},confirmButton={TextButton(onClick={contextRow=null}){Text("关闭")}})}
}

@Composable private fun InputKeyDialog(vm:EditorViewModel,key:KeyTarget,onDismiss:()->Unit) {
    var target by remember(key){mutableStateOf(key.frame.toString())}
    AlertDialog(onDismissRequest=onDismiss,title={Text("关键帧 "+key.frame)},text={Column {
        OutlinedTextField(target,{target=it},label={Text("目标帧")})
        TextButton(onClick={target.toIntOrNull()?.let{vm.copyKey(key.frame,it)};onDismiss()}){Text("复制到目标帧")}
    }},confirmButton={TextButton(onClick={target.toIntOrNull()?.let{vm.moveKeyFor(key.objectId,key.property,key.frame,it)};onDismiss()}){Text("精确移动")}},
        dismissButton={TextButton(onClick={vm.deleteKey(key.frame);onDismiss()}){Text("删除")}})
}
