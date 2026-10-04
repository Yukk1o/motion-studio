package com.motionstudio.editor

import android.os.Bundle
import android.content.Intent
import android.content.Context
import android.net.Uri
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.result.contract.ActivityResultContract
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
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale
import kotlin.math.*

internal val Background=Color(0xFF171A26)
internal val Panel=Color(0xFF202431)
internal val Accent=Color(0xFF37D4BE)
internal val Ink=Color(0xFFE6EAF2)
internal val Muted=Color(0xFF9DA6B7)
private class CreateOutputDocument(private val mime:String):ActivityResultContract<String,Uri?>() {
    override fun createIntent(context:Context,input:String)=Intent(Intent.ACTION_CREATE_DOCUMENT)
        .addCategory(Intent.CATEGORY_OPENABLE).setType(mime).putExtra(Intent.EXTRA_TITLE,input)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    override fun parseResult(resultCode:Int,intent:Intent?):Uri?=if(resultCode==android.app.Activity.RESULT_OK)intent?.data else null
}

open class MainActivity:ComponentActivity() {
    protected open fun initialProjectDirectory():File?=null
    private val model:EditorViewModel by viewModels {object:ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T:ViewModel> create(modelClass:Class<T>):T {
            require(modelClass==EditorViewModel::class.java)
            return EditorViewModel(application,initialProjectDirectory()) as T
        }
    }}
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {StudioTheme {Editor(model)}}
    }
    override fun onStart(){super.onStart();model.resumePreview()}
    override fun onStop() { model.suspendPreview();super.onStop() }
}

@Composable internal fun StudioTheme(content:@Composable ()->Unit) {
    MaterialTheme(colorScheme=darkColorScheme(primary=Accent,background=Background,surface=Panel,
        onSurface=Ink,onBackground=Ink),content=content)
}

@Composable internal fun Tool(icon:ImageVector,label:String,enabled:Boolean=true,action:()->Unit) {
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
    var library by remember{mutableStateOf(false)}
    val imagePicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let(vm::importImage)}
    val projectPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let(vm::importProject)}
    val pngSave=rememberLauncherForActivityResult(CreateOutputDocument("image/png")){uri->
        vm.completeOutputSelection(uri)
    }
    val projectSave=rememberLauncherForActivityResult(CreateOutputDocument("application/zip")){uri->
        vm.completeOutputSelection(uri)
    }
    val videoSave=rememberLauncherForActivityResult(CreateOutputDocument("video/mp4")){uri->
        vm.completeOutputSelection(uri)
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
                            DropdownMenuItem(text={Text("视频 MP4")},onClick={outputMenu=false;vm.exportVideo{vm.pendingOutput=it;vm.pendingOutputKind="video";videoSave.launch("MotionStudio.mp4")}})
                            DropdownMenuItem(text={Text("当前帧 PNG")},onClick={outputMenu=false;vm.output(true){vm.pendingOutput=it;vm.pendingOutputKind="png";pngSave.launch("motion-frame.png")}})
                            DropdownMenuItem(text={Text("备份工程")},onClick={outputMenu=false;vm.output(false){vm.pendingOutput=it;vm.pendingOutputKind="project";projectSave.launch("MotionStudio.motion")}})
                        }
                    }
                }
                Preview(vm,Modifier.weight(1f).fillMaxWidth())
                Transport(vm)
                Timeline(vm,Modifier.fillMaxWidth().height(timelineHeight))
                EditorFooter(vm)
            }
            if(!vm.panelOpen) {
                Box(Modifier.align(Alignment.BottomEnd).padding(end=16.dp,bottom=64.dp)) {
                    OutlinedIconButton(onClick={addMenu=true},modifier=Modifier.size(54.dp),
                        border=BorderStroke(1.5.dp,Accent)) {Icon(Icons.Default.Add,"添加图层",tint=Accent)}
                    DropdownMenu(addMenu,{addMenu=false}) {
                        DropdownMenuItem(text={Text("矩形")},onClick={addMenu=false;vm.addRectangle()})
                        DropdownMenuItem(text={Text("图片")},onClick={addMenu=false;imagePicker.launch(arrayOf("image/png","image/jpeg"))})
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
            if(vm.state.project==null&&vm.loadFailed&&!vm.state.busy) {
                Column(Modifier.fillMaxSize().background(Background).padding(24.dp),verticalArrangement=Arrangement.Center,
                    horizontalAlignment=Alignment.CenterHorizontally) {
                    Text("工程无法打开",color=Ink,fontSize=20.sp)
                    Spacer(Modifier.height(12.dp))
                    Text("原工程和素材已保留。可以恢复素材后重试，或导入备份。",color=Muted,fontSize=13.sp)
                    TextButton(onClick=vm::retryOpen){Text("重试打开")}
                    TextButton(onClick={vm.refreshProjects();library=true}){Text("打开其他工程")}
                    TextButton(onClick={projectPicker.launch(arrayOf("application/zip","application/octet-stream"))}){Text("导入备份")}
                    TextButton(onClick={vm.newProject(1080,1920,30)}){Text("新建工程")}
                }
            }else if(vm.state.busy||vm.state.project==null) {
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha=.4f)),contentAlignment=Alignment.Center) {
                    CircularProgressIndicator(color=Accent)
                }
            }
        }
    }
    vm.state.error?.let{message->AlertDialog(onDismissRequest=vm::clearError,
        title={Text("操作未完成")},text={Text(message)},
        confirmButton={TextButton(onClick=vm::clearError){Text("知道了")}},
        dismissButton={if(vm.pendingOutput!=null&&vm.outputPhase.startsWith("failed"))TextButton(onClick={vm.clearError()
            when(vm.pendingOutputKind){"video"->videoSave.launch("MotionStudio.mp4");"project"->projectSave.launch("MotionStudio.motion");else->pngSave.launch("motion-frame.png")}
        }){Text("重新选择位置")}else if(vm.state.project!=null)TextButton(onClick=vm::retryPreview){Text("重试预览")}})}
    if(textDialog)InputDialog("添加文字","Motion Studio",onDismiss={textDialog=false}){vm.addText(it);textDialog=false}
    if(vm.exporting)AlertDialog(onDismissRequest={},title={Text("导出视频")},
        text={Column{LinearProgressIndicator(progress={vm.exportProgress},modifier=Modifier.fillMaxWidth(),color=Accent)
            Spacer(Modifier.height(12.dp));Text((vm.exportProgress*100).toInt().toString()+"% · 本机编码")}},
        confirmButton={},dismissButton={TextButton(onClick=vm::cancelExport){Text("取消导出")}})
    if(settings)AlertDialog(onDismissRequest={settings=false},title={Text("Motion Studio · 合成")},
        text={Column {
            Text(vm.state.project?.let{it.getInt("width").toString()+" × "+it.getInt("height")+"\n"+
                it.getInt("fps")+" fps · "+String.format(Locale.US,"%.2f",it.getInt("frames").toDouble()/it.getInt("fps"))+" 秒"}?:"加载中")
            TextButton(onClick={settings=false;vm.refreshProjects();library=true}){Text("打开工程")}
            TextButton(onClick={settings=false;projectPicker.launch(arrayOf("application/zip","application/octet-stream"))}){Text("导入工程")}
            Text("新建 6 秒合成",fontSize=12.sp,color=Muted)
            listOf(1080 to 1920,1920 to 1080,1080 to 1080).forEach{(w,h)->
                Row {
                    TextButton(onClick={settings=false;vm.newProject(w,h,30)},modifier=Modifier.testTag("new-"+w+"-"+h+"-30")){Text(w.toString()+"×"+h+" / 30")}
                    TextButton(onClick={settings=false;vm.newProject(w,h,60)},modifier=Modifier.testTag("new-"+w+"-"+h+"-60")){Text("60 fps")}
                }
            }
        }},
        confirmButton={TextButton(onClick={settings=false}){Text("完成")}})
    if(library)AlertDialog(onDismissRequest={library=false},title={Text("打开工程")},
        text={Column(Modifier.heightIn(max=400.dp).verticalScroll(rememberScrollState())) {
            if(vm.projects.isEmpty())Text("暂无其他已保存工程",color=Muted)
            vm.projects.forEach{project->TextButton(onClick={library=false;vm.openProject(project.directory)},modifier=Modifier.fillMaxWidth().heightIn(min=56.dp)) {
                Column(Modifier.fillMaxWidth()) {
                    Text(project.name,color=Ink)
                    Text(project.width.toString()+" × "+project.height+" · "+project.fps+" fps",color=Muted,fontSize=11.sp)
                }
            }}
        }},confirmButton={TextButton(onClick={library=false}){Text("关闭")}})
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
        Canvas(Modifier.fillMaxSize()) {
            if(!vm.playing&&vm.selected!=0L) {
                previewPolygons(vm,size.width,size.height).firstOrNull{it.first==vm.selected}?.second?.let{points->
                    val outline=Path().apply{moveTo(points[0].x,points[0].y);points.drop(1).forEach{lineTo(it.x,it.y)};close()}
                    drawPath(outline,Accent,style=androidx.compose.ui.graphics.drawscope.Stroke(1.dp.toPx()))
                    points.forEach{drawRect(Ink,Offset(it.x-3.dp.toPx(),it.y-3.dp.toPx()),Size(6.dp.toPx(),6.dp.toPx()))}
                }
            }
        }
        Box(Modifier.fillMaxSize().testTag("preview-gesture").pointerInput(Unit) {
            awaitEachGesture {
                val down=awaitFirstDown()
                val selectedCorners=previewPolygons(vm,size.width.toFloat(),size.height.toFloat()).firstOrNull{it.first==vm.selected}?.second
                val handleRadius=selectedCorners?.let{points->min(20.dp.toPx(),points.indices.minOf{(points[it]-points[(it+1)%points.size]).getDistance()}/3f)}?:0f
                val resize=!vm.state.observing&&selectedCorners?.any{(it-down.position).getDistance()<=handleRadius}==true
                val picked=if(!resize&&!vm.state.observing)previewPolygons(vm,size.width.toFloat(),size.height.toFloat()).asReversed()
                    .firstOrNull{insideQuad(down.position,it.second)}else null
                if(vm.selected!=0L)picked?.let{vm.select(it.first,false)}
                var total=Offset.Zero;var active=false
                val at=floor(vm.frame).toInt();val objectId=vm.selected
                var scale=vm.sampleValueFor(objectId,"scale") as? JSONArray
                val initialScale=scale?.let{JSONArray(it.toString())}
                val anchor=previewAnchor(vm,objectId,size.width.toFloat(),size.height.toFloat())
                val startRadius=anchor?.let{(down.position-it).getDistance()}?:0f
                var rotation=vm.sampleValueFor(objectId,"rotation") as? JSONArray
                var azimuth=(vm.sampleValueFor(0,"azimuth") as? Number)?.toDouble()?:0.0
                var elevation=(vm.sampleValueFor(0,"elevation") as? Number)?.toDouble()?:0.0
                val cameraOrbit=vm.state.project?.getJSONObject("camera")?.getString("mode")=="orbit"
                val observing=vm.state.observing
                try {
                    do {
                        val event=awaitPointerEvent()
                        val pan=event.calculatePan();total+=pan
                        val zoom=event.calculateZoom();val angle=event.calculateRotation()
                        var startedNow=false
                        if(!active&&(total.getDistance()>viewConfiguration.touchSlop||abs(zoom-1f)>.002f||abs(angle)>.1f)) {
                            if(vm.editable()||observing){if(!observing)vm.beginGesture();active=true;startedNow=true}
                        }
                        if(active) {
                            val movement=if(startedNow)total else pan
                            if(observing)vm.navigate(movement.x,movement.y,zoom,event.changes.count{it.pressed}>1,size.width,size.height)
                            else if(objectId==0L) {
                                if(abs(zoom-1f)>.002f)vm.dolly((zoom-1f)*800f)
                                if(movement.getDistance()>.1f) {
                                    if(cameraOrbit){azimuth+=movement.x*.18;elevation=(elevation-movement.y*.18).coerceIn(-89.0,89.0);vm.recordOrbit(azimuth,elevation,at)}
                                    else vm.pan(movement.x*3f,-movement.y*3f)
                                }
                            } else {
                                if(resize&&anchor!=null&&initialScale!=null&&startRadius>1f) {
                                    val ratio=(event.changes.first().position-anchor).getDistance()/startRadius
                                    scale=JSONArray(initialScale.toString()).put(0,(initialScale.getDouble(0)*ratio).coerceIn(-100000.0,100000.0))
                                        .put(1,(initialScale.getDouble(1)*ratio).coerceIn(-100000.0,100000.0))
                                    vm.setPropertyValue(objectId,"scale",at,scale!!,false)
                                } else if(movement.getDistance()>.1f)vm.moveLayer(movement.x,movement.y,size.width,size.height)
                                if(!resize&&abs(zoom-1f)>.002f)scale?.let{old->
                                    scale=JSONArray(old.toString()).put(0,(old.getDouble(0)*zoom).coerceIn(-100000.0,100000.0))
                                        .put(1,(old.getDouble(1)*zoom).coerceIn(-100000.0,100000.0))
                                    vm.setPropertyValue(objectId,"scale",at,scale!!,false)
                                }
                                if(!resize&&abs(angle)>.1f)rotation?.let{old->
                                    rotation=JSONArray(old.toString()).put(2,old.getDouble(2)+angle)
                                    vm.setPropertyValue(objectId,"rotation",at,rotation!!,false)
                                }
                            }
                            event.changes.forEach{it.consume()}
                        }
                    } while(event.changes.any{it.pressed})
                } finally {if(active&&!observing)vm.endGesture()}
                if(!active&&!observing)picked?.let{vm.select(it.first,false)}
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
        Text(if(vm.state.observing)"观察 · 不录入" else if(vm.selected==0L) {
            if(vm.state.project?.getJSONObject("camera")?.getString("mode")=="orbit")"摄影机 · 环绕" else "摄影机 · 平移"
        }else"图层 · 变换",Modifier.align(Alignment.BottomStart).padding(10.dp),color=Muted,fontSize=10.sp)
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
        Tool(Icons.Default.Diamond,if(vm.currentKey()==null)"添加关键帧" else "删除当前关键帧",vm.editable(),vm::toggleKey)
        Tool(Icons.Default.CropFree,"观察视图"){vm.observe(!vm.state.observing)}
    }
}

@Composable internal fun InputDialog(title:String,initial:String,onDismiss:()->Unit,onConfirm:(String)->Unit) {
    var value by remember(initial){mutableStateOf(initial)}
    AlertDialog(onDismissRequest=onDismiss,title={Text(title)},
        text={OutlinedTextField(value,{value=it},singleLine=true)},
        confirmButton={TextButton(onClick={onConfirm(value)}){Text("确定")}},
        dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}
