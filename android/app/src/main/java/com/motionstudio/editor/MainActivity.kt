package com.motionstudio.editor

import android.os.Bundle
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.*
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.*
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale
import kotlin.math.*

private val Background=Color(0xFF171A26)
private val Panel=Color(0xFF202431)
private val Accent=Color(0xFF37D4BE)
private val Ink=Color(0xFFE6EAF2)
private val Muted=Color(0xFF9DA6B7)

class MainActivity:ComponentActivity() {
    private val model:EditorViewModel by viewModels()
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {StudioTheme {Editor(model)}}
    }
    override fun onStop() { model.pause();super.onStop() }
}

@Composable internal fun StudioTheme(content:@Composable ()->Unit) {
    MaterialTheme(colorScheme=darkColorScheme(primary=Accent,background=Background,surface=Panel,
        onSurface=Ink,onBackground=Ink),content=content)
}

@Composable private fun Tool(icon:ImageVector,label:String,enabled:Boolean=true,action:()->Unit) {
    IconButton(onClick=action,enabled=enabled,modifier=Modifier.size(48.dp)) {
        Icon(icon,label,tint=if(enabled)Ink else Muted.copy(alpha=.35f),modifier=Modifier.size(22.dp))
    }
}
@Composable internal fun Editor(vm:EditorViewModel) {
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    var addMenu by remember{mutableStateOf(false)}
    var outputMenu by remember{mutableStateOf(false)}
    var settings by remember{mutableStateOf(false)}
    var textDialog by remember{mutableStateOf(false)}
    var pendingFile by remember{mutableStateOf<File?>(null)}
    val imagePicker=rememberLauncherForActivityResult(ActivityResultContracts.GetContent()){uri->uri?.let(vm::importImage)}
    val projectPicker=rememberLauncherForActivityResult(ActivityResultContracts.GetContent()){uri->uri?.let(vm::importProject)}
    val pngSave=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")){uri->
        val file=pendingFile
        if(uri!=null&&file!=null)scope.launch(Dispatchers.IO){context.contentResolver.openOutputStream(uri)?.use{out->file.inputStream().use{it.copyTo(out)}}}
    }
    val projectSave=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")){uri->
        val file=pendingFile
        if(uri!=null&&file!=null)scope.launch(Dispatchers.IO){context.contentResolver.openOutputStream(uri)?.use{out->file.inputStream().use{it.copyTo(out)}}}
    }
    val videoSave=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("video/mp4")){uri->
        val file=pendingFile
        if(uri!=null&&file!=null)scope.launch(Dispatchers.IO){context.contentResolver.openOutputStream(uri)?.use{out->file.inputStream().use{it.copyTo(out)}}}
    }
    BackHandler(enabled=vm.panelOpen){vm.panelOpen=false}
    Surface(color=Background,modifier=Modifier.fillMaxSize()) {
        BoxWithConstraints(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.systemBars)) {
            val wide=maxWidth>maxHeight
            val availableHeight=maxHeight
            val availableWidth=maxWidth
            val timelineHeight=if(wide)(availableHeight*.3f).coerceAtMost(132.dp)
                else (availableHeight*.38f).coerceAtMost(300.dp)
            Column(Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
                    Tool(Icons.AutoMirrored.Filled.ArrowBack,"收起属性"){vm.panelOpen=false}
                    Text(vm.state.project?.optString("name")?:"Motion Studio",modifier=Modifier.weight(1f),fontSize=15.sp)
                    Tool(Icons.Default.Settings,"合成设置"){settings=true}
                    Box {
                        Tool(Icons.Default.IosShare,"输出"){outputMenu=true}
                        DropdownMenu(outputMenu,{outputMenu=false}) {
                            DropdownMenuItem(text={Text("视频 MP4")},onClick={outputMenu=false;vm.exportVideo{pendingFile=it;videoSave.launch("MotionStudio.mp4")}})
                            DropdownMenuItem(text={Text("当前帧 PNG")},onClick={outputMenu=false;vm.output(true){pendingFile=it;pngSave.launch("motion-frame.png")}})
                            DropdownMenuItem(text={Text("备份工程")},onClick={outputMenu=false;vm.output(false){pendingFile=it;projectSave.launch("MotionStudio.motion")}})
                        }
                    }
                }
                Preview(vm,Modifier.weight(1f).fillMaxWidth())
                Transport(vm)
                Timeline(vm,Modifier.fillMaxWidth().height(timelineHeight))
                Row(Modifier.fillMaxWidth().height(52.dp).background(Panel).padding(horizontal=16.dp),
                    verticalAlignment=Alignment.CenterVertically) {
                    Icon(if(vm.selected==0L)Icons.Default.Videocam else Icons.Default.Layers,null,Modifier.size(22.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(if(vm.selected==0L)"摄影机 1" else vm.layer(vm.selected)?.optString("name")?:"图层",Modifier.weight(1f),fontSize=13.sp)
                    TextButton(onClick={vm.panelOpen=true}){Text("编辑参数",color=Accent,fontSize=12.sp)}
                }
            }
            if(!vm.panelOpen) {
                Box(Modifier.align(Alignment.BottomEnd).padding(end=16.dp,bottom=64.dp)) {
                    OutlinedIconButton(onClick={addMenu=true},modifier=Modifier.size(54.dp),
                        border=BorderStroke(1.5.dp,Accent)) {Icon(Icons.Default.Add,"添加图层",tint=Accent)}
                    DropdownMenu(addMenu,{addMenu=false}) {
                        DropdownMenuItem(text={Text("矩形")},onClick={addMenu=false;vm.addRectangle()})
                        DropdownMenuItem(text={Text("图片")},onClick={addMenu=false;imagePicker.launch("image/*")})
                        DropdownMenuItem(text={Text("文字")},onClick={addMenu=false;textDialog=true})
                    }
                }
            }
            // This sibling overlays the stable editor; it never changes Surface
            // or timeline constraints and adds no scrim over the preview.
            AnimatedVisibility(visible=vm.panelOpen,
                modifier=Modifier.align(if(wide)Alignment.BottomEnd else Alignment.BottomCenter),
                enter=if(wide)slideInHorizontally{it}+fadeIn() else slideInVertically{it}+fadeIn(),
                exit=if(wide)slideOutHorizontally{it}+fadeOut() else slideOutVertically{it}+fadeOut()) {
                Properties(vm,if(wide)Modifier.width((availableWidth*.5f).coerceIn(280.dp,320.dp).coerceAtMost(availableWidth)).height((availableHeight-48.dp).coerceAtLeast(0.dp))
                    else Modifier.fillMaxWidth().height((availableHeight*.42f).coerceAtMost(304.dp)
                        .coerceAtMost(timelineHeight+8.dp)))
            }
            if(vm.state.busy||vm.state.project==null) {
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha=.4f)),contentAlignment=Alignment.Center) {
                    CircularProgressIndicator(color=Accent)
                }
            }
        }
    }
    vm.state.error?.let{message->AlertDialog(onDismissRequest=vm::clearError,
        title={Text("操作未完成")},text={Text(message)},
        confirmButton={TextButton(onClick=vm::clearError){Text("知道了")}})}
    if(textDialog)InputDialog("添加文字","Motion Studio",onDismiss={textDialog=false}){vm.addText(it);textDialog=false}
    if(vm.exporting)AlertDialog(onDismissRequest={},title={Text("导出视频")},
        text={Column{LinearProgressIndicator(progress={vm.exportProgress},modifier=Modifier.fillMaxWidth(),color=Accent)
            Spacer(Modifier.height(12.dp));Text((vm.exportProgress*100).toInt().toString()+"% · 本机编码")}},
        confirmButton={},dismissButton={TextButton(onClick=vm::cancelExport){Text("取消导出")}})
    if(settings)AlertDialog(onDismissRequest={settings=false},title={Text("Motion Studio · 合成")},
        text={Column {
            Text(vm.state.project?.let{it.getInt("width").toString()+" × "+it.getInt("height")+"\n"+
                it.getInt("fps")+" fps · "+String.format(Locale.US,"%.2f",it.getInt("frames").toDouble()/it.getInt("fps"))+" 秒"}?:"加载中")
            TextButton(onClick={settings=false;projectPicker.launch("*/*")}){Text("导入工程")}
            Text("新建 6 秒合成",fontSize=12.sp,color=Muted)
            listOf(1080 to 1920,1920 to 1080,1080 to 1080).forEach{(w,h)->
                Row {
                    TextButton(onClick={settings=false;vm.newProject(w,h,30)}){Text(w.toString()+"×"+h+" / 30")}
                    TextButton(onClick={settings=false;vm.newProject(w,h,60)}){Text("60 fps")}
                }
            }
        }},
        confirmButton={TextButton(onClick={settings=false}){Text("完成")}})
}

@Composable private fun Preview(vm:EditorViewModel,modifier:Modifier) {
    var menu by remember{mutableStateOf(false)}
    Box(modifier.background(Color(0xFF10121A))) {
        AndroidView(factory={context->SurfaceView(context).also{view->
            view.holder.addCallback(object:SurfaceHolder.Callback {
                override fun surfaceCreated(holder:SurfaceHolder) {}
                override fun surfaceChanged(holder:SurfaceHolder,format:Int,width:Int,height:Int){vm.attach(holder.surface,width,height)}
                override fun surfaceDestroyed(holder:SurfaceHolder){vm.detach()}
            })
        }},modifier=Modifier.fillMaxSize())
        Box(Modifier.fillMaxSize().testTag("preview-gesture").pointerInput(vm.selected,vm.state.observing) {
            awaitEachGesture {
                awaitFirstDown(requireUnconsumed=false)
                vm.beginGesture()
                do {
                    val event=awaitPointerEvent()
                    val pan=event.calculatePan()
                    val zoom=event.calculateZoom()
                    if(pan.getDistance()>0.1f||abs(zoom-1f)>.002f) {
                        if(vm.state.observing)vm.observe(true,pan.x*.18,pan.y*.18)
                        else if(vm.selected==0L) {
                            if(abs(zoom-1f)>.002f)vm.dolly((zoom-1f)*800f)
                            if(pan.getDistance()>.1f)vm.pan(pan.x*3f,-pan.y*3f)
                        } else {
                            vm.moveLayer(pan.x,pan.y,size.width,size.height)
                        }
                        event.changes.forEach{it.consume()}
                    }
                } while(event.changes.any{it.pressed})
                vm.endGesture()
            }
        })
        Box(Modifier.padding(start=12.dp,top=4.dp)) {
            TextButton(onClick={menu=true},modifier=Modifier.heightIn(min=48.dp)) {
                Text(if(vm.state.observing)"空间观察" else "成片摄影机",color=Ink,fontSize=12.sp)
                Icon(Icons.Default.ArrowDropDown,null,Modifier.size(18.dp))
            }
            DropdownMenu(menu,{menu=false}) {
                DropdownMenuItem(text={Text("成片摄影机")},onClick={menu=false;vm.view(0)})
                DropdownMenuItem(text={Text("空间观察")},onClick={menu=false;vm.view(1)})
                DropdownMenuItem(text={Text("顶视")},onClick={menu=false;vm.view(2)})
                DropdownMenuItem(text={Text("侧视")},onClick={menu=false;vm.view(3)})
            }
        }
    }
}
@Composable private fun Transport(vm:EditorViewModel) {
    Row(Modifier.fillMaxWidth().height(48.dp).testTag("transport").padding(horizontal=8.dp),
        horizontalArrangement=Arrangement.SpaceBetween,verticalAlignment=Alignment.CenterVertically) {
        Tool(Icons.AutoMirrored.Filled.Undo,"撤销",vm.state.canUndo,vm::undo)
        Tool(Icons.AutoMirrored.Filled.Redo,"重做",vm.state.canRedo,vm::redo)
        Tool(Icons.Default.SkipPrevious,"上一帧"){vm.step(-1)}
        Tool(if(vm.playing)Icons.Default.Pause else Icons.Default.PlayArrow,"播放/暂停",action=vm::togglePlay)
        Tool(Icons.Default.SkipNext,"下一帧"){vm.step(1)}
        Tool(Icons.Default.Diamond,"添加关键帧",action=vm::addKey)
        Tool(Icons.Default.CropFree,"观察视图"){vm.observe(!vm.state.observing)}
    }
}

private data class TimelineRow(val id:Long,val name:String,val color:Color,val visible:Boolean,val locked:Boolean,val track:JSONObject?)
@Composable private fun Timeline(vm:EditorViewModel,modifier:Modifier) {
    val context=LocalContext.current
    val density=context.resources.displayMetrics.density
    val p=vm.state.project
    val rows=buildList {
        if(p!=null) {
            add(TimelineRow(0,"摄影机 1",Color(0xFFE5C17E),true,false,p.getJSONObject("camera").optJSONObject(if(vm.selected==0L)vm.property else "position")))
            val layers=p.getJSONArray("layers")
            for(i in layers.length()-1 downTo 0) {
                val l=layers.getJSONObject(i)
                add(TimelineRow(l.getLong("id"),l.getString("name"),listOf(Color(0xFF6EADE8),Color(0xFFAD9DE0),Color(0xFF67BFAF))[i%3],
                    l.getBoolean("visible"),l.getBoolean("locked"),l.getJSONObject("transform").optJSONObject(if(vm.selected==l.getLong("id"))vm.property else "position")))
            }
        }
    }
    var vertical by remember{mutableFloatStateOf(0f)}
    var editKey by remember{mutableStateOf<Int?>(null)}
    Canvas(modifier.testTag("timeline").pointerInput(rows,vm.timelineScale) {
        detectTapGestures(onTap={pos->
            val r=((pos.y-44*density+vertical)/(52*density)).toInt()
            if(pos.y>=44*density&&r in rows.indices) {
                val row=rows[r]
                if(pos.x<48*density&&row.id!=0L)vm.flags(row.id,!row.visible,row.locked)
                else {
                    val scale=vm.timelineScale*density
                    val key=row.track?.optJSONArray("keys")?.let{a->(0 until a.length()).map{a.getJSONObject(it).getInt("frame")}
                        .firstOrNull{abs(size.width/2f+(it-vm.frame)*scale-pos.x)<16*density}}
                    vm.select(row.id);key?.let{vm.seek(it.toDouble())}
                }
            }
        },onLongPress={pos->
            val r=((pos.y-44*density+vertical)/(52*density)).toInt()
            if(r in rows.indices) {
                val row=rows[r]
                val key=row.track?.optJSONArray("keys")?.let{a->(0 until a.length()).map{a.getJSONObject(it).getInt("frame")}
                    .firstOrNull{abs(size.width/2f+(it-vm.frame)*vm.timelineScale*density-pos.x)<24*density}}
                if(key!=null){vm.select(row.id);editKey=key}
            }
        })
    }.pointerInput(vm.timelineScale,rows.size) {
        detectTransformGestures{_,pan,zoom,_->
            if(abs(zoom-1)>0.01f)vm.timelineScale=(vm.timelineScale*zoom).coerceIn(.4f,12f)
            else if(abs(pan.x)>abs(pan.y))vm.seek(vm.frame-pan.x/(vm.timelineScale*density))
            else vertical=(vertical-pan.y).coerceIn(0f,max(0f,rows.size*52*density-size.height+44*density))
        }
    }) {
        val rowHeight=52*density;val head=44*density
        val center=size.width/2;val scale=vm.timelineScale*density
        val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply{color=android.graphics.Color.LTGRAY;textSize=11*density}
        val startFrame=max(0,(vm.frame-center/scale).toInt())
        val endFrame=min((p?.optInt("frames")?:180),ceil(vm.frame+center/scale).toInt())
        for(f in startFrame..endFrame) {
            val x=center+(f-vm.frame).toFloat()*scale
            if(f%5==0)drawLine(Muted.copy(alpha=.5f),Offset(x,0f),Offset(x,if(f%30==0)13*density else 7*density),density)
        }
        val fps=p?.optInt("fps")?:30
        val current=floor(vm.frame).toInt()
        val time=String.format(Locale.US,"%02d:%02d:%02d",current/fps/60,current/fps%60,current%fps)
        drawRoundRect(Panel,Offset(center-48*density,16*density),Size(96*density,23*density),androidx.compose.ui.geometry.CornerRadius(4*density))
        paint.textAlign=android.graphics.Paint.Align.CENTER
        drawContext.canvas.nativeCanvas.drawText(time,center,32*density,paint)
        clipRect(top=head) {
        rows.forEachIndexed{index,row->
            val y=head+index*rowHeight-vertical
            if(y+rowHeight<head||y>size.height)return@forEachIndexed
            val x=center-vm.frame.toFloat()*scale
            val length=(p?.optInt("frames")?:180)*scale
            drawRoundRect(row.color.copy(alpha=if(row.visible)1f else .3f),Offset(max(49*density,x),y+7*density),
                Size(max(0f,min(size.width-12*density,x+length)-max(49*density,x)),30*density),androidx.compose.ui.geometry.CornerRadius(4*density))
            paint.color=android.graphics.Color.rgb(23,33,41);paint.textAlign=android.graphics.Paint.Align.LEFT;paint.textSize=12*density
            drawContext.canvas.nativeCanvas.drawText(row.name,max(58*density,x+12*density),y+27*density,paint)
            drawCircle(if(row.visible)Ink else Muted,8*density,Offset(23*density,y+25*density),style=androidx.compose.ui.graphics.drawscope.Stroke(1.4f*density))
            drawCircle(Ink,2*density,Offset(23*density,y+25*density))
            row.track?.optJSONArray("keys")?.let{a->
                for(i in 0 until a.length()) {
                    val f=a.getJSONObject(i).getInt("frame");val kx=center+(f-vm.frame).toFloat()*scale
                    val ky=y+43*density
                    val path=Path().apply{moveTo(kx,ky-5*density);lineTo(kx+5*density,ky);lineTo(kx,ky+5*density);lineTo(kx-5*density,ky);close()}
                    drawPath(path,Accent)
                }
            }
        }
        }
        drawLine(Ink.copy(alpha=.65f),Offset(center,head),Offset(center,size.height),density)
    }
    editKey?.let{key->var target by remember(key){mutableStateOf(key.toString())}
        AlertDialog(onDismissRequest={editKey=null},title={Text("关键帧 "+key)},
            text={Column {
                OutlinedTextField(target,{target=it},label={Text("目标帧")})
                TextButton(onClick={target.toIntOrNull()?.let{vm.copyKey(key,it)};editKey=null}){Text("复制到目标帧")}
            }},
            confirmButton={TextButton(onClick={target.toIntOrNull()?.let{vm.moveKey(key,it)};editKey=null}){Text("移动")}},
            dismissButton={TextButton(onClick={vm.deleteKey(key);editKey=null}){Text("删除")}})
    }
}

@Composable private fun Properties(vm:EditorViewModel,modifier:Modifier) {
    var rename by remember{mutableStateOf(false)}
    var more by remember{mutableStateOf(false)}
    var ease by remember{mutableStateOf(false)}
    val camera=vm.selected==0L
    val mode=vm.state.project?.optJSONObject("camera")?.optString("mode")?:"position"
    val choices=if(camera) {
        if(mode=="orbit")listOf("radius" to "距离","azimuth" to "方位","elevation" to "俯仰","target" to "目标点","fov" to "视角","roll" to "滚转")
        else listOf("position" to "位置","target" to "目标点","fov" to "视角","roll" to "滚转")
    } else listOf("position" to "位置","rotation" to "旋转","scale" to "缩放","opacity" to "透明度")
    Column(modifier.testTag("properties-panel").background(Panel,RoundedCornerShape(topStart=12.dp,topEnd=12.dp))
        .pointerInput(Unit) {
            // This pointer node claims the panel's hit region over its siblings.
            // Leave events unconsumed so child controls and scrolling still work.
            awaitPointerEventScope {while(true)awaitPointerEvent()}
        }.padding(horizontal=16.dp)) {
        Box(Modifier.fillMaxWidth().height(12.dp),contentAlignment=Alignment.Center){Box(Modifier.size(36.dp,3.dp).background(Muted.copy(alpha=.4f),RoundedCornerShape(2.dp)))}
        Row(Modifier.fillMaxWidth().height(48.dp),verticalAlignment=Alignment.CenterVertically) {
            Text(if(camera)"摄影机 1" else vm.layer(vm.selected)?.optString("name")?:"图层",Modifier.weight(1f),fontSize=16.sp)
            TextButton(onClick=vm::animate){Text(if(vm.keys().isEmpty())"动画 关" else "动画 开",fontSize=12.sp,color=Accent)}
            Box {
                Tool(Icons.Default.MoreVert,"图层操作"){more=true}
                DropdownMenu(more,{more=false}) {
                    if(camera)DropdownMenuItem(text={Text(if(mode=="orbit")"切换位置路径" else "切换环绕轨道")},onClick={more=false;vm.cameraMode(mode!="orbit");vm.property=if(mode=="orbit")"position" else "radius"})
                    else {
                        DropdownMenuItem(text={Text("重命名")},onClick={more=false;rename=true})
                        DropdownMenuItem(text={Text("复制图层")},onClick={more=false;vm.duplicate()})
                        DropdownMenuItem(text={Text("上移图层")},onClick={more=false;vm.reorder(1)})
                        DropdownMenuItem(text={Text("下移图层")},onClick={more=false;vm.reorder(-1)})
                        DropdownMenuItem(text={Text("锁定 / 解锁")},onClick={more=false;vm.layer(vm.selected)?.let{vm.flags(vm.selected,it.getBoolean("visible"),!it.getBoolean("locked"))}})
                        DropdownMenuItem(text={Text("删除图层")},onClick={more=false;vm.deleteLayer()})
                    }
                }
            }
            Tool(Icons.Default.Close,"关闭属性面板"){vm.panelOpen=false}
        }
        Column(Modifier.weight(1f).fillMaxWidth().testTag("property-values").verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            choices.forEach{(key,label)->TextButton(onClick={vm.pause();vm.property=key},modifier=Modifier.height(48.dp)){
                Text(label,fontSize=12.sp,color=if(vm.property==key)Accent else Muted)
            }}
        }
        val sampled=vm.sampleValue()
        if(sampled is JSONArray) {
            Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                for(i in 0 until min(3,sampled.length())) {
                    NumericField(listOf("X","Y","Z")[i],sampled.optDouble(i),Modifier.weight(1f)) {value->
                        val arr=JSONArray(sampled.toString());arr.put(i,value);vm.setValue(arr)
                    }
                }
            }
        } else if(sampled is Number) {
            NumericField(choices.firstOrNull{it.first==vm.property}?.second?:"数值",sampled.toDouble(),Modifier.fillMaxWidth()) {vm.setValue(it)}
        }
        Row(Modifier.fillMaxWidth().height(44.dp),verticalAlignment=Alignment.CenterVertically) {
            Text("点按输入精确值",Modifier.weight(1f),fontSize=11.sp,color=Muted)
            TextButton(onClick={ease=true}){Text("缓动",fontSize=12.sp,color=Accent)}
        }
        }
        Row(Modifier.fillMaxWidth().height(56.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
            Tool(Icons.Default.SkipPrevious,"上一关键帧"){vm.jumpKey(false)}
            Button(onClick=vm::addKey,modifier=Modifier.weight(1f).height(48.dp),shape=RoundedCornerShape(5.dp)){Text("+ 添加关键帧",fontSize=12.sp,color=Background)}
            Tool(Icons.Default.SkipNext,"下一关键帧"){vm.jumpKey(true)}
        }
    }
    if(rename)InputDialog("图层名称",vm.layer(vm.selected)?.optString("name")?:"",onDismiss={rename=false}){vm.rename(it);rename=false}
    if(ease)AlertDialog(onDismissRequest={ease=false},title={Text("关键帧缓动")},
        text={Column{listOf("linear" to "线性","in" to "缓入","out" to "缓出","in_out" to "缓入缓出","hold" to "保持").forEach{(key,label)->
            TextButton(onClick={vm.ease(key);ease=false},modifier=Modifier.fillMaxWidth()){Text(label)}
        }}},confirmButton={TextButton(onClick={ease=false}){Text("取消")}})
}
@Composable private fun NumericField(label:String,value:Double,modifier:Modifier,onValue:(Double)->Unit) {
    var edit by remember{mutableStateOf(false)}
    Column(modifier) {
        Text(label,fontSize=11.sp,color=Muted,modifier=Modifier.height(20.dp))
        Surface(color=Background,shape=RoundedCornerShape(5.dp),modifier=Modifier.fillMaxWidth().height(48.dp).clickable{edit=true}) {
            Box(contentAlignment=Alignment.Center){Text(String.format(Locale.US,"%.1f",value),fontSize=15.sp,fontFamily=FontFamily.Monospace)}
        }
    }
    if(edit)InputDialog(label,String.format(Locale.US,"%.3f",value),onDismiss={edit=false}) {input->
        val number=input.replace(',','.').toDoubleOrNull()
        if(number!=null&&number.isFinite()){onValue(number);edit=false}
    }
}
@Composable private fun InputDialog(title:String,initial:String,onDismiss:()->Unit,onConfirm:(String)->Unit) {
    var value by remember(initial){mutableStateOf(initial)}
    AlertDialog(onDismissRequest=onDismiss,title={Text(title)},
        text={OutlinedTextField(value,{value=it},singleLine=true)},
        confirmButton={TextButton(onClick={onConfirm(value)}){Text("确定")}},
        dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}
