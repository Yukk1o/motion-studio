package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.text.input.KeyboardType
import org.json.JSONObject
import java.util.Locale
import kotlin.math.*

private data class TrackRow(val id:Long,val name:String,val color:Color,val visible:Boolean,val locked:Boolean,
    val property:String,val axis:Int?,val keys:List<Int>,val allKeys:Set<Int>,val start:Int,val end:Int,val parent:Long?=null)
private data class KeyTarget(val objectId:Long,val property:String,val axis:Int?,val frame:Int)
private data class ClipDraft(val objectId:Long,val start:Int,val end:Int,val mode:String)

@Composable internal fun Timeline(vm:EditorViewModel,modifier:Modifier) {
    val density=LocalDensity.current.density
    val project=vm.state.project
    val frames=project?.optInt("frames")?:180
    val rows=remember(project,vm.state.sample,vm.selected,vm.property,vm.activeAxis()){buildList {
        if(project!=null) {
            fun keys(track:JSONObject?):List<Int> = track?.optJSONArray("keys")?.let{a->
                (0 until a.length()).map{a.getJSONObject(it).getLong("frame")}.filter{it in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()}.map{it.toInt()}}?:emptyList()
            fun allKeys(track:JSONObject?):Set<Int> = if(track?.has("axes")==true)listOf("x","y","z").flatMap{keys(track.getJSONObject("axes").optJSONObject(it))}.toSet()else keys(track).toSet()
            fun rowKeys(id:Long,key:String):Pair<Int?,List<Int>> {
                val t=vm.propertyTrack(id,key)
                val axis=if(t?.has("axes")==true)if(id==vm.selected)vm.activeAxis()else if(key=="rotation")2 else 0 else null
                return axis to keys(if(axis!=null)t?.getJSONObject("axes")?.optJSONObject(vm.axisName(axis))else t)
            }
            val camera=project.getJSONObject("camera")
            val cameraKey=if(vm.selected==0L)vm.property else if(camera.getString("mode")=="orbit")"radius"else"position"
            if(camera.optBoolean("created",true)) {
                val (axis,keyFrames)=rowKeys(0,cameraKey)
                val all=camera.keys().asSequence().flatMap{allKeys(vm.propertyTrack(0,it)).asSequence()}.toSet()
                add(TrackRow(0,"摄影机 1",Color(0xFFE5C17E),true,false,cameraKey,axis,keyFrames,all,0,frames))
            }
            val layers=project.getJSONArray("layers")
            for(i in layers.length()-1 downTo 0) {
                val l=layers.getJSONObject(i);val id=l.getLong("id");val key=if(vm.selected==id)vm.property else "position"
                val (axis,keyFrames)=rowKeys(id,key)
                val all=listOf("position","rotation","scale","opacity").flatMap{allKeys(vm.propertyTrack(id,it))}.toSet()
                val clip=vm.timelineLayer(id)
                add(TrackRow(id,l.getString("name"),listOf(Color(0xFF6EADE8),Color(0xFFAD9DE0),Color(0xFF67BFAF))[i%3],
                    l.getBoolean("visible"),l.getBoolean("locked"),key,axis,keyFrames,all,clip?.optInt("in_frame")?:0,clip?.optInt("out_frame")?:frames,parentOf(vm,id)))
            }
        }
    }}
    val currentRows by rememberUpdatedState(rows)
    var vertical by remember{mutableFloatStateOf(0f)}
    var movedKey by remember{mutableStateOf<KeyTarget?>(null)}
    var editKey by remember{mutableStateOf<KeyTarget?>(null)}
    var clipDraft by remember{mutableStateOf<ClipDraft?>(null)}
    var contextRow by remember{mutableStateOf<TrackRow?>(null)}
    var trimRow by remember{mutableStateOf<TrackRow?>(null)}
    var moveRow by remember{mutableStateOf<TrackRow?>(null)}
    var jumpDialog by remember{mutableStateOf(false)}
    var hoveredRow by remember{mutableStateOf<Int?>(null)}
    Canvas(modifier.testTag("timeline").pointerInput(Unit) {
        var lastTapId=-1L;var lastTapTime=0L
        awaitEachGesture {
            val down=awaitFirstDown()
            val values=currentRows
            val rowHeight=52*density;val head=44*density
            val totalFrames=vm.state.project?.optInt("frames")?:180
            val startFrame=vm.frame;val startScale=vm.timelineScale*density;val startScroll=vertical
            val rowIndex=floor((down.position.y-head+vertical)/rowHeight).toInt()
            val row=if(down.position.y>=head)values.getOrNull(rowIndex)else null
            val rulerRow=values.firstOrNull{it.id==vm.selected}
            val keyRow=if(down.position.y<16*density)rulerRow else if(row!=null&&down.position.y>=head+rowIndex*rowHeight-vertical+32*density)row else null
            val hitKey=keyRow?.keys?.minByOrNull{abs(size.width/2f+(it-startFrame)*startScale-down.position.x)}
                ?.takeIf{abs(size.width/2f+(it-startFrame)*startScale-down.position.x)<=18*density}
            fun x(frame:Int)=size.width/2f+(frame-startFrame).toFloat()*startScale
            val insideClip=row!=null&&down.position.x>=max(49*density,x(row.start))&&down.position.x<=x(row.end)&&down.position.x<size.width-12*density
            val edge=if(row!=null&&row.id!=0L&&row.id==vm.selected&&!row.locked&&down.position.x>=49*density) {
                listOf("trim-start" to abs(down.position.x-x(row.start)),"trim-end" to abs(down.position.x-x(row.end)))
                    .minByOrNull{it.second}?.takeIf{it.second<=24*density}?.first
            }else null
            var total=Offset.Zero;var mode="";var target=hitKey;var gesture=false;var completed=false
            try {
                do {
                    val event=awaitPointerEvent()
                    if(event.changes.any{it.isConsumed})break
                    val pan=event.calculatePan();total+=pan
                    val elapsed=event.changes.first().uptimeMillis-down.uptimeMillis
                    if(event.changes.count{it.pressed}>1) {
                        if(gesture){vm.cancelGesture();gesture=false;clipDraft=null}
                        mode="zoom"
                        val center=event.calculateCentroid(useCurrent=true)
                        val old=vm.timelineScale*density
                        val anchor=vm.frame+(center.x-size.width/2f)/old
                        val next=(vm.timelineScale*event.calculateZoom()).coerceIn(.4f,12f)
                        vm.timelineScale=next
                        vm.seek((anchor-(center.x-size.width/2f)/(next*density)).roundToInt().toDouble())
                    }else if(mode.isEmpty()&&total.getDistance()>viewConfiguration.touchSlop) {
                        mode=when {
                            hitKey!=null&&keyRow?.locked==false->"key"
                            edge!=null&&abs(total.x)>=abs(total.y)->edge
                            row!=null&&row.id!=0L&&!row.locked&&elapsed>=viewConfiguration.longPressTimeoutMillis&&abs(total.y)>abs(total.x)->"reorder"
                            row!=null&&row.id!=0L&&!row.locked&&insideClip&&elapsed>=viewConfiguration.longPressTimeoutMillis->"move"
                            abs(total.x)>=abs(total.y)->"scrub"
                            else->"scroll"
                        }
                        if(mode=="key"&&keyRow!=null){vm.select(keyRow.id,false);vm.property=keyRow.property;keyRow.axis?.let{vm.chooseAxis(it)}}
                        if(mode in listOf("move","trim-start","trim-end")&&row!=null){vm.select(row.id,false);vm.beginGesture();gesture=true}
                    }
                    when(mode) {
                        "key"->if(keyRow!=null&&hitKey!=null) {
                            target=(hitKey+total.x/startScale).roundToInt().coerceIn(0,totalFrames-1)
                            movedKey=KeyTarget(keyRow.id,keyRow.property,keyRow.axis,target)
                        }
                        "scrub"->vm.seek((startFrame-total.x/startScale).roundToInt().toDouble())
                        "scroll"->vertical=(startScroll-total.y).coerceIn(0f,max(0f,values.size*rowHeight-size.height+head))
                        "reorder"->hoveredRow=floor((down.position.y+total.y-head+vertical)/rowHeight).toInt().coerceIn(if(values.firstOrNull()?.id==0L)1 else 0,values.lastIndex)
                        "move","trim-start","trim-end"->if(row!=null) {
                            val delta=(total.x/startScale).roundToInt()
                            val start=if(mode=="move")(row.start+delta).coerceIn(0,totalFrames-(row.end-row.start))else if(mode=="trim-start")(row.start+delta).coerceIn(0,row.end-1)else row.start
                            val end=if(mode=="move")start+row.end-row.start else if(mode=="trim-end")(row.end+delta).coerceIn(row.start+1,totalFrames)else row.end
                            val next=ClipDraft(row.id,start,end,mode)
                            if(next!=clipDraft) {
                                clipDraft=next
                                if(mode=="move")vm.moveClip(row.id,start,false)else vm.trimClip(row.id,start,end,false)
                            }
                        }
                    }
                    completed=event.changes.none{it.pressed}
                    if(mode.isNotEmpty())event.changes.forEach{it.consume()}
                }while(!completed)
                if(completed&&mode=="key"&&keyRow!=null&&hitKey!=null&&target!=hitKey) {
                    vm.moveKeyFor(keyRow.id,keyRow.property,hitKey,target!!,keyRow.axis);vm.seek(target!!.toDouble())
                }else if(completed&&mode=="reorder"&&row!=null&&hoveredRow!=null) {
                    val cameraCount=if(values.firstOrNull()?.id==0L)1 else 0
                    vm.select(row.id,false);vm.reorderTo(row.id,values.size-cameraCount-1-(hoveredRow!!-cameraCount))
                }else if(completed&&mode.isEmpty()) {
                    val duration=currentEvent.changes.first().uptimeMillis-down.uptimeMillis
                    if(hitKey!=null&&keyRow!=null) {
                        vm.select(keyRow.id,false);vm.property=keyRow.property;keyRow.axis?.let{vm.chooseAxis(it)};vm.seek(hitKey.toDouble())
                        if(duration>=viewConfiguration.longPressTimeoutMillis)editKey=KeyTarget(keyRow.id,keyRow.property,keyRow.axis,hitKey)
                    }else if(row!=null) {
                        if(down.position.x<48*density&&row.id!=0L)vm.flags(row.id,!row.visible,row.locked)
                        else {
                            val doubleTap=lastTapId==row.id&&down.uptimeMillis-lastTapTime<=viewConfiguration.doubleTapTimeoutMillis
                            vm.select(row.id,doubleTap)
                            if(duration>=viewConfiguration.longPressTimeoutMillis)contextRow=row
                            lastTapId=row.id;lastTapTime=down.uptimeMillis
                        }
                    }else if(down.position.y<head) {
                        if(duration>=viewConfiguration.longPressTimeoutMillis)jumpDialog=true
                        else vm.seek((startFrame+(down.position.x-size.width/2f)/startScale).roundToInt().toDouble())
                    }
                }
            }finally {
                if(gesture){if(completed)vm.endGesture()else vm.cancelGesture()}
                movedKey=null;hoveredRow=null;clipDraft=null
            }
        }
    }) {
        val rowHeight=52*density;val head=44*density;val center=size.width/2f;val scale=vm.timelineScale*density
        val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply{color=android.graphics.Color.LTGRAY;textSize=11*density}
        val startFrame=max(0,(vm.frame-center/scale).toInt());val endFrame=min(frames,ceil(vm.frame+center/scale).toInt())
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
        fun keyMarks(keys:Collection<Int>,y:Float,active:Collection<Int>) {
            val bin=max(1,ceil(14*density/scale).toInt())
            keys.filter{it in 0 until frames}.sorted().groupBy{it/bin}.values.forEach{group->
                if(group.size==1)diamond(group.first(),y,group.first() in active)
                else {
                    val x=center+(group.average()-vm.frame).toFloat()*scale
                    drawCircle(if(group.any{it in active})Accent.copy(alpha=.65f)else Muted.copy(alpha=.35f),2.5f*density,Offset(x,y))
                    group.firstOrNull{it in active&&abs(it-vm.frame)<.5}?.let{diamond(it,y,true)}
                }
            }
        }
        rows.firstOrNull{it.id==vm.selected}?.let{keyMarks(it.keys,9*density,it.keys)}
        movedKey?.let{diamond(it.frame,9*density,true)}
        val fps=project?.optInt("fps")?:30;val current=floor(vm.frame).toInt()
        val time=clipDraft?.let{"${it.start} → ${it.end} 帧"}?:String.format(Locale.US,"%02d:%02d:%02d",current/fps/60,current/fps%60,current%fps)
        drawRoundRect(Panel,Offset(center-48*density,17*density),Size(96*density,22*density),androidx.compose.ui.geometry.CornerRadius(4*density))
        paint.textAlign=android.graphics.Paint.Align.CENTER
        drawContext.canvas.nativeCanvas.drawText(time,center,32*density,paint)
        clipRect(top=head) {
            rows.forEachIndexed{index,row->
                val y=head+index*rowHeight-vertical
                if(y+rowHeight<head||y>size.height)return@forEachIndexed
                val draft=clipDraft?.takeIf{it.objectId==row.id}
                val begin=draft?.start?:row.start;val end=draft?.end?:row.end
                val x=center+(begin-vm.frame).toFloat()*scale;val right=center+(end-vm.frame).toFloat()*scale
                val left=max(49*density,x);val width=max(0f,min(size.width-12*density,right)-left)
                if(row.id==vm.selected)drawRect(Accent.copy(alpha=.08f),Offset(0f,y),Size(size.width,rowHeight))
                if(width>1) {
                    drawRoundRect(row.color.copy(alpha=if(row.visible).18f else .06f),Offset(left,y+7*density),Size(width,30*density),androidx.compose.ui.geometry.CornerRadius(6*density))
                    if(row.id==vm.selected)drawRoundRect(Accent.copy(alpha=.65f),Offset(left,y+7*density),Size(width,30*density),androidx.compose.ui.geometry.CornerRadius(6*density),style=Stroke(density))
                    drawRoundRect(row.color,Offset(left+3*density,y+13*density),Size(min(3*density,width),18*density),androidx.compose.ui.geometry.CornerRadius(1.5f*density))
                    if(row.id==vm.selected&&row.id!=0L&&!row.locked)for(edge in listOf(x+4*density,right-4*density)) {
                        if(edge in 49*density..size.width-12*density)drawLine(Accent,Offset(edge,y+15*density),Offset(edge,y+29*density),2*density)
                    }
                }
                paint.color=(if(row.visible)Ink else Muted).let{android.graphics.Color.argb(255,(it.red*255).toInt(),(it.green*255).toInt(),(it.blue*255).toInt())}
                paint.textAlign=android.graphics.Paint.Align.LEFT;paint.textSize=12*density
                paint.typeface=if(row.id==vm.selected)android.graphics.Typeface.DEFAULT_BOLD else android.graphics.Typeface.DEFAULT
                if(width>24*density)clipRect(left=left+10*density,top=y+7*density,right=left+width-6*density,bottom=y+37*density) {
                    drawContext.canvas.nativeCanvas.drawText((if(row.parent!=null)"↳ "else"")+row.name,left+12*density,y+27*density,paint)
                }else drawContext.canvas.nativeCanvas.drawText(row.name,58*density,y+27*density,paint)
                val eye=Path().apply{moveTo(12*density,y+25*density);quadraticTo(23*density,y+10*density,34*density,y+25*density);quadraticTo(23*density,y+40*density,12*density,y+25*density)}
                drawPath(eye,if(row.visible)Ink else Muted,style=Stroke(1.3f*density))
                if(row.visible)drawCircle(Ink,3*density,Offset(23*density,y+25*density))
                keyMarks(row.allKeys,y+44*density,if(row.id==vm.selected)row.keys else emptyList())
                if(row.locked)drawRect(Muted,Offset(size.width-19*density,y+16*density),Size(7*density,8*density))
            }
            hoveredRow?.let{index->drawLine(Accent,Offset(48*density,head+index*rowHeight-vertical),Offset(size.width,head+index*rowHeight-vertical),2*density)}
        }
        drawLine(Ink.copy(alpha=.65f),Offset(center,head),Offset(center,size.height),density)
    }
    editKey?.let{key->InputKeyDialog(vm,key){editKey=null}}
    if(jumpDialog)InputDialog("跳到帧",floor(vm.frame).toInt().toString(),onDismiss={jumpDialog=false}){it.toIntOrNull()?.let{frame->vm.seek(frame.toDouble())};jumpDialog=false}
    moveRow?.let{row->ClipMoveDialog(vm,row){moveRow=null}}
    trimRow?.let{row->ClipTrimDialog(vm,row){trimRow=null}}
    contextRow?.let{row->AlertDialog(onDismissRequest={contextRow=null},title={Text(row.name)},text={Column(Modifier.verticalScroll(rememberScrollState())) {
        TextButton(onClick={vm.select(row.id);contextRow=null}){Text("移动和变换")}
        if(row.id!=0L) {
            TextButton(enabled=!row.locked,onClick={moveRow=row;contextRow=null}){Text("精确移动片段")}
            TextButton(enabled=!row.locked,onClick={trimRow=row;contextRow=null}){Text("精确裁剪片段")}
            TextButton(enabled=!row.locked&&floor(vm.frame).toInt() in row.start+1 until row.end,onClick={vm.splitClip();contextRow=null}){Text("在当前帧分割")}
            Row {
                TextButton(enabled=!row.locked,onClick={vm.reorder(1);contextRow=null}){Text("上移图层")}
                TextButton(enabled=!row.locked,onClick={vm.reorder(-1);contextRow=null}){Text("下移图层")}
            }
            TextButton(onClick={vm.duplicate();contextRow=null}){Text("复制图层")}
            TextButton(onClick={vm.flags(row.id,row.visible,!row.locked);contextRow=null}){Text(if(row.locked)"解锁图层"else"锁定图层")}
            TextButton(enabled=!row.locked,onClick={vm.deleteLayer();contextRow=null}){Text("删除图层")}
        }
    }},confirmButton={TextButton(onClick={contextRow=null}){Text("关闭")}})}
}

@Composable private fun ClipMoveDialog(vm:EditorViewModel,row:TrackRow,onDismiss:()->Unit) {
    var input by remember{mutableStateOf(row.start.toString())}
    val frame=input.toIntOrNull();val maximum=(vm.state.project?.optInt("frames")?:0)-(row.end-row.start)
    val valid=frame!=null&&frame in 0..maximum
    AlertDialog(onDismissRequest=onDismiss,title={Text("移动片段")},text={
        OutlinedTextField(input,{input=it},label={Text("开始帧")},singleLine=true,isError=!valid,
            supportingText={Text("范围 0–$maximum 帧 · 时长保持不变")},keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number))
    },confirmButton={TextButton(enabled=valid,onClick={vm.moveClip(row.id,frame!!);onDismiss()}){Text("应用移动")}},
        dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}

@Composable private fun ClipTrimDialog(vm:EditorViewModel,row:TrackRow,onDismiss:()->Unit) {
    var start by remember{mutableStateOf(row.start.toString())};var end by remember{mutableStateOf(row.end.toString())}
    val a=start.toIntOrNull();val b=end.toIntOrNull();val frames=vm.state.project?.optInt("frames")?:0
    val valid=a!=null&&b!=null&&a>=0&&b<=frames&&a<b
    AlertDialog(onDismissRequest=onDismiss,title={Text("裁剪片段")},text={Column(Modifier.verticalScroll(rememberScrollState())) {
        OutlinedTextField(start,{start=it},label={Text("开始帧")},singleLine=true,isError=!valid,
            keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number),modifier=Modifier.testTag("clip-in"))
        OutlinedTextField(end,{end=it},label={Text("结束帧（不含）")},singleLine=true,isError=!valid,
            supportingText={if(!valid)Text("请输入 0–$frames 帧内的有效区间")},
            keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number),modifier=Modifier.testTag("clip-out"))
        Text("保留完整关键帧和素材",color=Muted)
    }},confirmButton={TextButton(enabled=valid,onClick={vm.trimClip(row.id,a!!,b!!);onDismiss()}){Text("应用裁剪")}},dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}

@Composable private fun InputKeyDialog(vm:EditorViewModel,key:KeyTarget,onDismiss:()->Unit) {
    var target by remember(key){mutableStateOf(key.frame.toString())}
    AlertDialog(onDismissRequest=onDismiss,title={Text("关键帧 "+key.frame)},text={Column {
        OutlinedTextField(target,{target=it},label={Text("目标帧")})
        TextButton(onClick={target.toIntOrNull()?.let{vm.copyKeyFor(key.objectId,key.property,key.frame,it,key.axis)};onDismiss()}){Text("复制到目标帧")}
    }},confirmButton={TextButton(onClick={target.toIntOrNull()?.let{vm.moveKeyFor(key.objectId,key.property,key.frame,it,key.axis)};onDismiss()}){Text("精确移动")}},
        dismissButton={TextButton(onClick={vm.deleteKeyFor(key.objectId,key.property,key.frame,key.axis);onDismiss()}){Text("删除")}})
}
