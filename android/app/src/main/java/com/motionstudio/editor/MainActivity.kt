package com.motionstudio.editor

import android.os.Bundle
import android.content.Intent
import android.content.Context
import android.net.Uri
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.MotionEvent
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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.*
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
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

internal val Background=Color(0xFF121519)
internal val Panel=Color(0xFF1C2128)
internal val Accent=Color(0xFF54DCC7)
internal val Ink=Color(0xFFEDF1F5)
internal val Muted=Color(0xFFAAB4C2)
private class CreateOutputDocument(private val mime:String):ActivityResultContract<String,Uri?>() {
    override fun createIntent(context:Context,input:String)=Intent(Intent.ACTION_CREATE_DOCUMENT)
        .addCategory(Intent.CATEGORY_OPENABLE).setType(mime).putExtra(Intent.EXTRA_TITLE,input)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
    override fun parseResult(resultCode:Int,intent:Intent?):Uri?=if(resultCode==android.app.Activity.RESULT_OK)intent?.data else null
}

open class MainActivity:ComponentActivity() {
    protected open fun initialProjectDirectory():File?=null
    protected open fun initialProjectJson():String=""
    private val model:EditorViewModel by viewModels {object:ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T:ViewModel> create(modelClass:Class<T>):T {
            require(modelClass==EditorViewModel::class.java)
            return EditorViewModel(application,initialProjectDirectory(),initialProjectJson()) as T
        }
    }}
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        val startAtHome=initialProjectDirectory()==null
        setContent {StudioTheme {Editor(model,startAtHome)}}
    }
    override fun onStart(){super.onStart();model.resumePreview()}
    override fun dispatchTouchEvent(event:MotionEvent):Boolean {
        if(event.actionMasked==MotionEvent.ACTION_DOWN)model.gestureInertia.stop()
        return super.dispatchTouchEvent(event)
    }
    override fun onStop() {model.gestureInertia.stop();model.suspendPreview();super.onStop()}
}

@Composable internal fun StudioTheme(content:@Composable ()->Unit) {
    MaterialTheme(colorScheme=darkColorScheme(primary=Accent,background=Background,surface=Panel,
        onSurface=Ink,onBackground=Ink),content=content)
}

@Composable internal fun Tool(icon:ImageVector,label:String,enabled:Boolean=true,action:()->Unit) {
    IconButton(onClick=action,enabled=enabled,modifier=Modifier.size(48.dp)) {
        Icon(editorIcon(icon),label,tint=if(enabled)Ink else Muted.copy(alpha=.35f),modifier=Modifier.size(22.dp))
    }
}
@Composable internal fun Editor(vm:EditorViewModel,startAtHome:Boolean=false) {
    val context=LocalContext.current
    val diagnosticSave=rememberLauncherForActivityResult(CreateOutputDocument("application/json"),vm::saveErrorReport)
    fun exportDiagnostic(){vm.prepareErrorReport{diagnosticSave.launch("MotionStudio-error-${System.currentTimeMillis()}.json")}}
    val scope=rememberCoroutineScope()
    var home by rememberSaveable{mutableStateOf(startAtHome)}
    var homePage by rememberSaveable{mutableStateOf("projects")}
    val homeState=rememberSaveableStateHolder()
    var addMenu by remember{mutableStateOf(false)}
    var shapeMenu by remember{mutableStateOf(false)}
    var compositionMenu by remember{mutableStateOf(false)}
    var compositionSettings by remember{mutableStateOf(false)}
    var releaseNotes by remember{mutableStateOf(false)}
    var outputMenu by remember{mutableStateOf(false)}
    var settings by remember{mutableStateOf(false)}
    var creating by remember{mutableStateOf(false)}
    var layoutEditing by rememberSaveable{mutableStateOf(false)}
    var textDialog by remember{mutableStateOf(false)}
    var library by remember{mutableStateOf(false)}
    var videoImport by remember{mutableStateOf(false)}
    var keepSound by remember{mutableStateOf(true)}
    var curveExpanded by remember{mutableStateOf(false)}
    val editorLayout=remember(vm){EditorLayoutState(vm.layoutPreferences)}
    var previewBounds by remember{mutableStateOf<androidx.compose.ui.geometry.Rect?>(null)}
    var editorOrigin by remember{mutableStateOf(Offset.Zero)}
    val imagePicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let(vm::importImage)}
    val projectPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let{vm.importProject(it);home=false}}
    val audioPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let{vm.importMedia(it,"audio")}}
    val videoPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let{vm.importMedia(it,"video",keepSound)}}
    val pluginPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->uri?.let(vm::installPlugin)}
    val pngSave=rememberLauncherForActivityResult(CreateOutputDocument("image/png")){uri->
        vm.completeOutputSelection(uri)
    }
    val projectSave=rememberLauncherForActivityResult(CreateOutputDocument("application/zip")){uri->
        vm.completeOutputSelection(uri)
    }
    val videoSave=rememberLauncherForActivityResult(CreateOutputDocument("video/mp4")){uri->
        vm.completeOutputSelection(uri)
    }
    fun finishLayout(){layoutEditing=false;homePage="settings";home=true}
    if(home) {
        fun backHomePage(){homePage=if(homePage=="plugins")"settings"else"projects"}
        BackHandler(enabled=homePage!="projects",onBack=::backHomePage)
        if(releaseNotes)ReleaseNotes(vm.updates){releaseNotes=false}
        homeState.SaveableStateProvider(homePage) {
            when(homePage) {
                "settings"->HomeSettings(vm,editorLayout,onBack=::backHomePage,onPackages={homePage="plugins"},onReport=::exportDiagnostic,onUpdates={releaseNotes=true},onAdjustLayout={
                    vm.gestureInertia.stop();vm.pause();vm.finishLayerSelection();layoutEditing=true;home=false
                })
                "plugins"->PluginSettings(vm,onInstall={pluginPicker.launch(arrayOf("application/zip","application/octet-stream","*/*"))},onBack=::backHomePage)
                else->ProjectHome(vm,onOpen={project->if(project.directory!=vm.root.name||vm.state.project==null)vm.openProject(project.directory);home=false},
                    onNew={w,h,fps,name,frames->vm.newProject(w,h,fps,name,frames);home=false},onImport={projectPicker.launch(arrayOf("application/zip","application/octet-stream"))},onSettings={homePage="settings"},onReport=::exportDiagnostic,onUpdates={releaseNotes=true})
            }
        }
        if(homePage!="projects")vm.state.error?.let{message->AlertDialog(onDismissRequest=vm::clearError,title={Text("操作未完成")},text={Column{Text(message);TextButton(onClick=::exportDiagnostic){Text("导出错误报告")}}},confirmButton={TextButton(onClick=vm::clearError){Text("知道了")}})}
        return
    }
    if(releaseNotes)ReleaseNotes(vm.updates){releaseNotes=false}
    BackHandler(enabled=vm.panelOpen&&!layoutEditing){vm.closeWorkspace()}
    BackHandler(enabled=!vm.panelOpen&&!vm.layerSelectionMode&&!layoutEditing){vm.pause();home=true}
    Surface(color=Background,modifier=Modifier.fillMaxSize().pointerInput(vm) {
        awaitPointerEventScope {while(true) {
            if(awaitPointerEvent(PointerEventPass.Initial).changes.any{it.changedToDownIgnoreConsumed()})vm.gestureInertia.stop()
        }}
    }) {
        BoxWithConstraints(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.systemBars).onGloballyPositioned{editorOrigin=it.positionInRoot()}) {
            val wide=maxWidth>maxHeight
            val availableHeight=maxHeight
            val availableWidth=maxWidth
            val split=wide&&availableWidth>=552.dp
            val sideWidth=if(split)editorLayout.value("landscape.side",availableWidth.value,
                (availableWidth*.48f).coerceIn(248.dp,320.dp).value,248f,(availableWidth-304.dp).value).dp else 0.dp
            val effectEditing=vm.panelOpen&&(vm.effectsOpen||vm.vectorOpen||vm.compositionClipOpen||vm.expressionTarget!=null)&&(vm.selected!=0L||vm.hasCamera())
            val effectWidth=if(split)sideWidth else editorLayout.value("landscape.side",availableWidth.value,
                (availableWidth*.5f).coerceAtMost(320.dp).value,200f,(availableWidth-256.dp).value).dp
            val dragDensity=LocalDensity.current.density
            val defaultFocusedHeight=48f+timelineRowHeightDp(LocalDensity.current.fontScale)
            val focusedTimelineHeight=if(wide)editorLayout.value("landscape.focused",availableHeight.value,
                defaultFocusedHeight,defaultFocusedHeight,(availableHeight-192.dp).value).dp else defaultFocusedHeight.dp
            val maxEffectHeight=(availableHeight-48.dp-48.dp-focusedTimelineHeight-96.dp).coerceAtLeast(144.dp)
            val effectHeight=editorLayout.value("portrait.effects",availableHeight.value,
                (availableHeight*(if(curveExpanded).52f else .44f)).coerceAtMost(360.dp).coerceAtMost(maxEffectHeight).value,
                144f,maxEffectHeight.value).dp
            fun closeEffects(){vm.closeWorkspace();vm.property="position"}
            val defaultTimelineHeight=if(wide)(availableHeight*.3f).coerceAtMost(132.dp)else (availableHeight*.38f).coerceAtMost(300.dp)
            val timelineHeight=editorLayout.value(if(wide)"landscape.timeline"else"portrait.timeline",availableHeight.value,
                defaultTimelineHeight.value,120f,(availableHeight-244.dp).value).dp
            val previewModifier=Modifier.onGloballyPositioned{previewBounds=it.boundsInRoot()}
            Column(Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
                    Tool(Icons.AutoMirrored.Filled.ArrowBack,if(layoutEditing)"完成布局调整" else if(vm.layerSelectionMode)"退出多选" else if(vm.panelOpen)"收起属性" else "工程列表") {
                        if(layoutEditing)finishLayout() else if(vm.layerSelectionMode)vm.finishLayerSelection() else if(vm.panelOpen)vm.closeWorkspace() else {vm.pause();home=true}
                    }
                    if(vm.compositionPath.size>1&&!layoutEditing&&!vm.layerSelectionMode)CompositionBreadcrumbs(vm,Modifier.weight(1f))
                    else Text(if(layoutEditing)"调整布局" else if(vm.layerSelectionMode)"选择图层"else vm.state.project?.optString("name")?:"Motion Studio",modifier=Modifier.weight(1f),fontSize=15.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                    if(layoutEditing)TextButton(onClick=::finishLayout,modifier=Modifier.height(48.dp).testTag("finish-layout")){Text("完成")}
                    else {
                        Tool(Icons.Default.Tune,"合成设置"){settings=true}
                        Box {
                            TextButton(onClick={outputMenu=true},modifier=Modifier.height(48.dp)) {
                                Icon(editorIcon(Icons.Default.IosShare),"输出",Modifier.size(18.dp),tint=Accent)
                                Spacer(Modifier.width(6.dp));Text("导出",color=Accent,fontSize=13.sp)
                            }
                            DropdownMenu(outputMenu,{outputMenu=false}) {
                                DropdownMenuItem(text={Text("视频 MP4")},onClick={outputMenu=false;vm.exportVideo{vm.pendingOutput=it;vm.pendingOutputKind="video";videoSave.launch("MotionStudio.mp4")}})
                                DropdownMenuItem(text={Text("当前帧 PNG")},onClick={outputMenu=false;vm.output(true){vm.pendingOutput=it;vm.pendingOutputKind="png";pngSave.launch("motion-frame.png")}})
                                DropdownMenuItem(text={Text("备份工程")},onClick={outputMenu=false;vm.output(false){vm.pendingOutput=it;vm.pendingOutputKind="project";projectSave.launch("MotionStudio.motion")}})
                            }
                        }
                    }
                }
                if(split||(wide&&effectEditing))Row(Modifier.weight(1f).fillMaxWidth()) {
                    Column(Modifier.weight(1f).fillMaxHeight()) {
                        Preview(vm,Modifier.weight(1f).fillMaxWidth().then(previewModifier))
                        Transport(vm)
                    }
                    Column(Modifier.width(if(effectEditing)effectWidth else sideWidth).fillMaxHeight()) {
                        if(effectEditing) {
                            if(vm.expressionTarget!=null)ExpressionWorkspace(vm,Modifier.fillMaxSize(),backEnabled=!layoutEditing)
                            else if(vm.compositionClipOpen)CompositionClipPanel(vm,Modifier.fillMaxSize())
                            else if(vm.vectorOpen)VectorPanel(vm,Modifier.fillMaxSize(),onCurveMode={curveExpanded=it})
                            else EffectsPanel(vm,Modifier.fillMaxSize(),backEnabled=!layoutEditing,onCurveMode={curveExpanded=it},onDismiss=::closeEffects)
                        }
                        else {
                            Timeline(vm,Modifier.weight(1f).fillMaxWidth())
                            EditorFooter(vm)
                        }
                    }
                }else {
                    Preview(vm,Modifier.weight(1f).fillMaxWidth().then(previewModifier))
                    Transport(vm)
                    Timeline(vm,Modifier.fillMaxWidth().height(if(effectEditing)focusedTimelineHeight else timelineHeight),focused=effectEditing)
                    if(effectEditing) {
                        if(vm.expressionTarget!=null)ExpressionWorkspace(vm,Modifier.fillMaxWidth().height(effectHeight),backEnabled=!layoutEditing)
                        else if(vm.compositionClipOpen)CompositionClipPanel(vm,Modifier.fillMaxWidth().height(effectHeight))
                        else if(vm.vectorOpen)VectorPanel(vm,Modifier.fillMaxWidth().height(effectHeight),onCurveMode={curveExpanded=it})
                        else EffectsPanel(vm,Modifier.fillMaxWidth().height(effectHeight),backEnabled=!layoutEditing,onCurveMode={curveExpanded=it},onDismiss=::closeEffects)
                    }
                    else EditorFooter(vm)
                }
                if(wide&&effectEditing)Timeline(vm,Modifier.fillMaxWidth().height(focusedTimelineHeight),focused=true)
            }
            if(!vm.panelOpen&&!vm.layerSelectionMode) {
                Box(Modifier.align(Alignment.BottomEnd).padding(end=16.dp,bottom=64.dp)) {
                    Button(onClick={vm.pause();addMenu=true},modifier=Modifier.height(48.dp).testTag("add-layer"),
                        shape=RoundedCornerShape(12.dp),colors=ButtonDefaults.buttonColors(containerColor=Accent,contentColor=Background)) {
                        Icon(editorIcon(Icons.Default.Add),"添加图层",Modifier.size(20.dp))
                        Spacer(Modifier.width(6.dp));Text("图层",fontSize=14.sp)
                    }
                }
            }
            // This sibling overlays the stable editor; it never changes Surface
            // or timeline constraints and adds no scrim over the preview.
            val propertyBounds=if(split)Modifier.width(sideWidth).height((availableHeight-48.dp-(if(curveExpanded)0.dp else 44.dp)).coerceAtLeast(0.dp))
                else if(wide)Modifier.width((availableWidth*.5f).coerceIn(280.dp,320.dp).coerceAtMost(availableWidth)).height((availableHeight-48.dp).coerceAtLeast(0.dp))
                else Modifier.fillMaxWidth().height(if(curveExpanded||vm.effectsOpen)(availableHeight*.64f).coerceAtMost(440.dp)
                    else if(editorLayout.custom("portrait.timeline"))timelineHeight+8.dp
                    else (availableHeight*.42f).coerceAtMost(304.dp).coerceAtMost(timelineHeight+8.dp))
            AnimatedVisibility(visible=vm.panelOpen&&!effectEditing&&(vm.selected!=0L||vm.hasCamera()),
                modifier=Modifier.align(if(wide)Alignment.BottomEnd else Alignment.BottomCenter),
                enter=if(wide)slideInHorizontally{it}+fadeIn() else slideInVertically{it}+fadeIn(),
                exit=if(wide)slideOutHorizontally{it}+fadeOut() else slideOutVertically{it}+fadeOut()) {
                Properties(vm,propertyBounds,backEnabled=!layoutEditing,onCurveMode={curveExpanded=it})
            }
            if(layoutEditing&&vm.state.project!=null)previewBounds?.let{bounds->
                if(split||(wide&&effectEditing))LayoutGrip(bounds,editorOrigin,true,"调整左右布局比例","layout-resize-side",editorLayout) {delta->
                    val current=if(effectEditing)effectWidth else sideWidth
                    editorLayout.set("landscape.side",(current.value-delta/dragDensity)/availableWidth.value)
                }
                if((!split||effectEditing)&&(!vm.panelOpen||vm.effectsOpen||!curveExpanded)) {
                    val key=when{wide&&effectEditing->"landscape.focused";effectEditing->"portrait.effects";wide->"landscape.timeline";else->"portrait.timeline"}
                    val current=when{wide&&effectEditing->focusedTimelineHeight;effectEditing->effectHeight;else->timelineHeight}
                    LayoutGrip(bounds,editorOrigin,false,"调整预览与下方面板比例","layout-resize-height",editorLayout) {delta->
                        editorLayout.set(key,(current.value-delta/dragDensity)/availableHeight.value)
                    }
                }
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
    if(addMenu)AddLayerSheet(hasCamera=vm.hasCamera(),onDismiss={addMenu=false},onAdd={kind->
        addMenu=false
        when(kind) {
            "image"->imagePicker.launch(arrayOf("image/png","image/jpeg"))
            "audio"->audioPicker.launch(arrayOf("audio/*","application/octet-stream"))
            "video"->videoImport=true
            "text"->textDialog=true
            "solid"->vm.addRectangle()
            "shapes"->shapeMenu=true
            "pen"->vm.addPenLayer()
            "adjustment"->vm.addAdjustment()
            "composition"->compositionMenu=true
            "null"->vm.addNull()
            "camera"->vm.addCamera()
        }
    })
    if(shapeMenu)ShapeCatalogue(vm){shapeMenu=false}
    if(compositionMenu)CompositionLibrary(vm){compositionMenu=false}
    if(compositionSettings)CompositionSettings(vm){compositionSettings=false}
    vm.state.error?.takeUnless{it.startsWith("expression ")}?.let{message->AlertDialog(onDismissRequest=vm::clearError,
        title={Text("操作未完成")},text={Column{Text(message);TextButton(onClick=::exportDiagnostic,modifier=Modifier.testTag("export-error-report")){Text("导出错误报告")};Text("包含设备信息和近期操作，不包含项目与素材。",color=Muted,fontSize=12.sp)}},
        confirmButton={TextButton(onClick=vm::clearError){Text("知道了")}},
        dismissButton={if(vm.pendingOutput!=null&&vm.outputPhase.startsWith("failed"))TextButton(onClick={vm.clearError()
            when(vm.pendingOutputKind){"video"->videoSave.launch("MotionStudio.mp4");"project"->projectSave.launch("MotionStudio.motion");else->pngSave.launch("motion-frame.png")}
        }){Text("重新选择位置")}else if(vm.state.project!=null)TextButton(onClick=vm::retryPreview){Text("重试预览")}})}
    if(textDialog)InputDialog("添加文字","Motion Studio",onDismiss={textDialog=false}){vm.addText(it);textDialog=false}
    if(videoImport)AlertDialog(onDismissRequest={videoImport=false},title={Text("导入视频")},text={Column{Text("支持 4K、最高 240 fps 与任意画幅导入，可选择 MP4、MOV、MKV、WebM、3GP 等视频。",color=Ink,modifier=Modifier.testTag("video-import-support"));Spacer(Modifier.height(8.dp));Text("横竖屏与宽高比不限；单边不超过 4096 像素，总像素不超过 4096 × 2160。当前支持 8 位 SDR，编码与解码能力取决于设备，选择后会检查。",color=Muted,fontSize=12.sp)
        Row(verticalAlignment=Alignment.CenterVertically){Checkbox(keepSound,{keepSound=it});Text("保留原声")}}},confirmButton={TextButton(onClick={videoImport=false;videoPicker.launch(arrayOf("video/*","application/octet-stream"))}){Text("选择视频")}},dismissButton={TextButton(onClick={videoImport=false}){Text("取消")}})
    vm.importTask?.let{task->AlertDialog(onDismissRequest={},title={Text("导入媒体")},text={Column {
        val phase=when(task.optString("phase")){"opening"->"打开文件";"copying"->"保存素材";"probing"->"检查画面";"decoding","decoding_audio"->"解码声音";"waveform"->"生成波形";"awaiting_commit"->"保存片段";else->"准备素材"}
        Text(phase);Spacer(Modifier.height(12.dp));LinearProgressIndicator(progress={task.optDouble("progress").toFloat().coerceIn(0f,1f)},modifier=Modifier.fillMaxWidth())
    }},confirmButton={},dismissButton={TextButton(onClick=vm::cancelMediaImport){Text("取消导入")}})}
    vm.mediaNotice?.let{notice->AlertDialog(onDismissRequest=vm::clearMediaNotice,title={Text("导入完成")},text={Text(notice)},confirmButton={TextButton(onClick=vm::clearMediaNotice){Text("知道了")}})}
    if(vm.exporting)AlertDialog(onDismissRequest={},title={Text("导出视频")},
        text={Column{LinearProgressIndicator(progress={vm.exportProgress},modifier=Modifier.fillMaxWidth(),color=Accent)
            Spacer(Modifier.height(12.dp));Text((vm.exportProgress*100).toInt().toString()+"% · 本机编码")}},
        confirmButton={},dismissButton={TextButton(onClick=vm::cancelExport){Text("取消导出")}})
    if(settings)AlertDialog(onDismissRequest={settings=false},title={Text("Motion Studio · 合成")},
        text={Column(Modifier.heightIn(max=400.dp).verticalScroll(rememberScrollState())) {
            Text(vm.state.project?.let{it.getInt("width").toString()+" × "+it.getInt("height")+"\n"+
                it.getInt("fps")+" fps · "+String.format(Locale.US,"%.2f",it.getInt("frames").toDouble()/it.getInt("fps"))+" 秒"}?:"加载中")
            TextButton(onClick={settings=false;vm.pause();vm.finishLayerSelection();home=true}){Text("打开工程")}
            TextButton(onClick={settings=false;compositionSettings=true},modifier=Modifier.testTag("edit-composition-settings")){Text("修改当前合成")}
            TextButton(onClick={settings=false;compositionMenu=true},modifier=Modifier.testTag("open-composition-library")){Text("管理子合成")}
            TextButton(onClick={settings=false;projectPicker.launch(arrayOf("application/zip","application/octet-stream"))}){Text("导入工程")}
            if(vm.state.project?.optJSONArray("audio_assets")?.length()?.let{it>0}==true||vm.state.project?.optJSONArray("video_assets")?.length()?.let{it>0}==true)TextButton(onClick={settings=false;vm.prepareMediaCaches(true)}){Text("重建媒体缓存")}
            Text("预览清晰度",fontSize=12.sp,color=Muted)
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                listOf("自动","清晰","流畅","省电").forEachIndexed{mode,label->
                    TextButton(onClick={vm.choosePreviewMode(mode)},modifier=Modifier.heightIn(min=48.dp).testTag("preview-mode-"+mode)) {
                        Text(label,color=if(vm.previewMode==mode)Accent else Muted)
                    }
                }
            }
            vm.previewInfo?.let{info->Text("预览 "+info.optInt("width")+" × "+info.optInt("height")+" · "+info.optInt("fps")+" fps",fontSize=11.sp,lineHeight=15.sp,color=Muted)}
            TextButton(onClick={settings=false;creating=true},modifier=Modifier.testTag("open-new-composition")){Text("自定义新建合成")}
            TextButton(onClick=::exportDiagnostic){Text("导出最近的错误报告")}
            Text("快速新建 6 秒合成",fontSize=12.sp,color=Muted)
            listOf(1080 to 1920,1920 to 1080,1080 to 1080).forEach{(w,h)->
                Row {
                    TextButton(onClick={settings=false;vm.newProject(w,h,30)},modifier=Modifier.testTag("new-"+w+"-"+h+"-30")){Text(w.toString()+"×"+h+" / 30")}
                    TextButton(onClick={settings=false;vm.newProject(w,h,60)},modifier=Modifier.testTag("new-"+w+"-"+h+"-60")){Text("60 fps")}
                }
            }
        }},
        confirmButton={TextButton(onClick={settings=false}){Text("完成")}})
    if(creating)NewProjectDialog(onDismiss={creating=false}){w,h,fps,name,frames->creating=false;vm.newProject(w,h,fps,name,frames)}
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
    // Panels suspend their Back handlers while adjusting layout.
    BackHandler(enabled=layoutEditing,onBack=::finishLayout)
}

@Composable private fun Preview(vm:EditorViewModel,modifier:Modifier) {
    var menu by remember{mutableStateOf(false)}
    Box(modifier.background(Color(0xFF0B0D10)).clipToBounds()) {
        AndroidView(factory={context->SurfaceView(context).also{view->
            view.holder.addCallback(object:SurfaceHolder.Callback {
                override fun surfaceCreated(holder:SurfaceHolder) {}
                override fun surfaceChanged(holder:SurfaceHolder,format:Int,width:Int,height:Int){vm.attach(holder.surface,width,height)}
                override fun surfaceDestroyed(holder:SurfaceHolder){vm.detach()}
            })
        }},modifier=Modifier.fillMaxSize())
        Canvas(Modifier.fillMaxSize().testTag("composition-boundary")) {
            val project=vm.state.project?:return@Canvas
            val fit=min(size.width/project.getInt("width"),size.height/project.getInt("height"))
            val width=project.getInt("width")*fit;val height=project.getInt("height")*fit
            val left=(size.width-width)/2;val top=(size.height-height)/2
            val surround=Color(0xFF303640)
            if(left>0) {
                drawRect(surround,Offset.Zero,Size(left,size.height))
                drawRect(surround,Offset(left+width,0f),Size(left,size.height))
            }
            if(top>0) {
                drawRect(surround,Offset.Zero,Size(size.width,top))
                drawRect(surround,Offset(0f,top+height),Size(size.width,top))
            }
            val stroke=1.dp.toPx()
            drawRect(Color(0xFF778291),Offset(left+stroke/2,top+stroke/2),
                Size((width-stroke).coerceAtLeast(0f),(height-stroke).coerceAtLeast(0f)),
                style=androidx.compose.ui.graphics.drawscope.Stroke(stroke))
        }
        vm.state.sample?.optString("renderError")?.takeIf{it.startsWith("expression ")}?.let{message->
            Text(message,Modifier.align(Alignment.BottomStart).padding(8.dp).background(Panel).padding(8.dp).testTag("expression-render-error"),
                color=MaterialTheme.colorScheme.error,fontSize=12.sp,maxLines=3,overflow=TextOverflow.Ellipsis)
        }
        Canvas(Modifier.fillMaxSize()) {
            if(!vm.playing&&vm.selected!=0L&&!vm.vectorOpen) {
                previewPolygons(vm,size.width,size.height).filter{if(vm.layerSelectionMode)it.first in vm.selectedLayerIds else it.first==vm.selected}.forEach{(_,points)->
                    val outline=Path().apply{moveTo(points[0].x,points[0].y);points.drop(1).forEach{lineTo(it.x,it.y)};close()}
                    drawPath(outline,Accent,style=androidx.compose.ui.graphics.drawscope.Stroke(1.dp.toPx()))
                    if(!vm.layerSelectionMode)points.forEach{drawRect(Ink,Offset(it.x-3.dp.toPx(),it.y-3.dp.toPx()),Size(6.dp.toPx(),6.dp.toPx()))}
                }
            }
        }
        Box(Modifier.fillMaxSize().testTag("preview-gesture").pointerInput(Unit) {
            awaitEachGesture {
                val down=awaitFirstDown()
                if(vm.layerSelectionMode) {
                    var travel=Offset.Zero;var ended=false
                    do {
                        val event=awaitPointerEvent();travel+=event.calculatePan();ended=event.changes.none{it.pressed}
                        event.changes.forEach{it.consume()}
                    }while(!ended)
                    if(travel.getDistance()<=viewConfiguration.touchSlop)vm.pickLayer(down.position.x,down.position.y,size.width.toFloat(),size.height.toFloat()){candidate->
                        if(vm.layerSelectionMode)candidate?.let(vm::toggleLayerSelection)
                    }
                    return@awaitEachGesture
                }
                val selectedCorners=previewPolygons(vm,size.width.toFloat(),size.height.toFloat()).firstOrNull{it.first==vm.selected}?.second
                val handleRadius=selectedCorners?.let{points->min(20.dp.toPx(),points.indices.minOf{(points[it]-points[(it+1)%points.size]).getDistance()}/3f)}?:0f
                val resize=!vm.state.observing&&selectedCorners?.any{(it-down.position).getDistance()<=handleRadius}==true
                var total=Offset.Zero;var active=false;var ended=false;var cancelled=false
                var pendingZoom=1f;var pendingAngle=0f
                val at=floor(vm.frame).toInt()
                var picked:Long?=null;var resolved=resize||vm.state.observing
                val cameraFocused=vm.selected==0L&&vm.hasCamera()
                if(!resize&&!vm.state.observing)vm.pickLayer(down.position.x,down.position.y,size.width.toFloat(),size.height.toFloat()){candidate->
                    resolved=true;picked=candidate
                    if(!cancelled&&!active&&(ended||!cameraFocused))candidate?.let{vm.select(it,false)}
                    // A short gesture may finish before the worker returns the
                    // hit result. Apply its accumulated motion to that target.
                    if(ended&&!cancelled&&!active&&!cameraFocused&&vm.editable()&&
                        (total.getDistance()>viewConfiguration.touchSlop||abs(pendingZoom-1f)>.002f||abs(pendingAngle)>.1f)) {
                        vm.beginGesture()
                        if(total.getDistance()>.1f)vm.moveLayer(total.x,total.y,size.width,size.height)
                        (vm.sampleValueFor(vm.selected,"scale") as? JSONArray)?.let{old->if(abs(pendingZoom-1f)>.002f)
                            vm.setPropertyValue(vm.selected,"scale",at,JSONArray(old.toString()).put(0,(old.getDouble(0)*pendingZoom).coerceIn(-100000.0,100000.0)).put(1,(old.getDouble(1)*pendingZoom).coerceIn(-100000.0,100000.0)),false,listOf(0,1))}
                        (vm.sampleValueFor(vm.selected,"rotation") as? JSONArray)?.let{old->if(abs(pendingAngle)>.1f)
                            vm.setPropertyValue(vm.selected,"rotation",at,JSONArray(old.toString()).put(2,old.getDouble(2)+pendingAngle),false,listOf(2))}
                        vm.endGesture();active=true
                    }
                }
                var objectId=vm.selected
                var scale=vm.sampleValueFor(objectId,"scale") as? JSONArray
                var initialScale=scale?.let{JSONArray(it.toString())}
                var anchor=previewAnchor(vm,objectId,size.width.toFloat(),size.height.toFloat())
                var startRadius=anchor?.let{(down.position-it).getDistance()}?:0f
                var rotation=vm.sampleValueFor(objectId,"rotation") as? JSONArray
                var azimuth=(vm.sampleValueFor(0,"azimuth") as? Number)?.toDouble()?:0.0
                var elevation=(vm.sampleValueFor(0,"elevation") as? Number)?.toDouble()?:0.0
                val cameraOrbit=vm.state.project?.getJSONObject("camera")?.getString("mode")=="orbit"
                val observing=vm.state.observing
                try {
                    do {
                        val event=awaitPointerEvent()
                        if(event.changes.any{it.isConsumed})break
                        val pan=event.calculatePan();total+=pan
                        var zoom=event.calculateZoom();var angle=event.calculateRotation()
                        if(!active){pendingZoom*=zoom;pendingAngle+=angle}
                        var startedNow=false
                        if(!active&&(resolved||cameraFocused)&&(total.getDistance()>viewConfiguration.touchSlop||abs(zoom-1f)>.002f||abs(angle)>.1f)) {
                            if(vm.editable()||observing){
                                objectId=vm.selected
                                scale=vm.sampleValueFor(objectId,"scale") as? JSONArray
                                rotation=vm.sampleValueFor(objectId,"rotation") as? JSONArray
                                initialScale=scale?.let{JSONArray(it.toString())}
                                anchor=previewAnchor(vm,objectId,size.width.toFloat(),size.height.toFloat())
                                startRadius=anchor?.let{(down.position-it).getDistance()}?:0f
                                if(!observing)vm.beginGesture();active=true;startedNow=true
                                zoom=pendingZoom;angle=pendingAngle
                            }
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
                                    vm.setPropertyValue(objectId,"scale",at,scale!!,false,listOf(0,1))
                                } else if(movement.getDistance()>.1f)vm.moveLayer(movement.x,movement.y,size.width,size.height)
                                if(!resize&&abs(zoom-1f)>.002f)scale?.let{old->
                                    scale=JSONArray(old.toString()).put(0,(old.getDouble(0)*zoom).coerceIn(-100000.0,100000.0))
                                        .put(1,(old.getDouble(1)*zoom).coerceIn(-100000.0,100000.0))
                                    vm.setPropertyValue(objectId,"scale",at,scale!!,false,listOf(0,1))
                                }
                                if(!resize&&abs(angle)>.1f)rotation?.let{old->
                                    rotation=JSONArray(old.toString()).put(2,old.getDouble(2)+angle)
                                    vm.setPropertyValue(objectId,"rotation",at,rotation!!,false,listOf(2))
                                }
                            }
                            event.changes.forEach{it.consume()}
                        }
                        ended=event.changes.none{it.pressed}
                    } while(!ended)
                } finally {if(!ended)cancelled=true;if(active&&!observing){if(ended)vm.endGesture()else vm.cancelGesture()}}
                if(!active&&!observing)picked?.let{vm.select(it,false)}
            }
        })
        if(vm.vectorOpen&&!vm.playing&&!vm.state.observing&&vm.vectorData()?.getJSONObject("source")?.optString("kind")=="paths")VectorPreviewOverlay(vm)
        Box(Modifier.padding(start=12.dp,top=4.dp)) {
            TextButton(onClick={menu=true},modifier=Modifier.heightIn(min=48.dp).background(Background.copy(alpha=.8f),RoundedCornerShape(10.dp))) {
                Text(if(vm.state.observing)"空间观察" else if(vm.hasCamera())"成片摄影机"else"合成视图",color=Ink,fontSize=12.sp)
                Icon(editorIcon(Icons.Default.ArrowDropDown),null,Modifier.size(18.dp))
            }
            DropdownMenu(menu,{menu=false}) {
                DropdownMenuItem(text={Text(if(vm.hasCamera())"成片摄影机"else"合成视图")},onClick={menu=false;vm.view(0)})
                DropdownMenuItem(text={Text("空间观察")},onClick={menu=false;vm.view(1)})
                DropdownMenuItem(text={Text("顶视")},onClick={menu=false;vm.view(2)})
                DropdownMenuItem(text={Text("侧视")},onClick={menu=false;vm.view(3)})
            }
        }
        if(vm.state.observing)Text("空间观察 · 不录入动画",Modifier.align(Alignment.BottomStart)
            .padding(10.dp).background(Background.copy(alpha=.8f),RoundedCornerShape(6.dp)).padding(6.dp),color=Muted,fontSize=12.sp)
    }
}
@Composable private fun Transport(vm:EditorViewModel) {
    var more by remember(vm.root){mutableStateOf(false)}
    BoxWithConstraints(Modifier.fillMaxWidth().height(48.dp).testTag("transport")) {
    val compact=maxWidth<352.dp
    Row(Modifier.fillMaxWidth().height(48.dp).padding(horizontal=8.dp),horizontalArrangement=Arrangement.SpaceBetween,verticalAlignment=Alignment.CenterVertically) {
        Row {
            Tool(Icons.AutoMirrored.Filled.Undo,"撤销",vm.state.canUndo,vm::undo)
            if(!compact)Tool(Icons.AutoMirrored.Filled.Redo,"重做",vm.state.canRedo,vm::redo)
        }
        Row(verticalAlignment=Alignment.CenterVertically) {
            Tool(Icons.Default.SkipPrevious,"上一帧"){vm.step(-1)}
            FilledIconButton(onClick=vm::togglePlay,modifier=Modifier.size(48.dp),
                colors=IconButtonDefaults.filledIconButtonColors(containerColor=Accent.copy(alpha=.14f),contentColor=Accent)) {
                Icon(editorIcon(if(vm.playing)Icons.Default.Pause else Icons.Default.PlayArrow),"播放/暂停",Modifier.size(26.dp))
            }
            Tool(Icons.Default.SkipNext,"下一帧"){vm.step(1)}
        }
        Row {
        Tool(Icons.Default.ContentCut,"切割图层",vm.canSplitClip(),vm::splitClip)
        Box {
            Tool(Icons.Default.MoreHoriz,"更多编辑操作"){more=true}
            DropdownMenu(more,{more=false}) {
                DropdownMenuItem(text={Text(if(vm.layerSelectionMode)"复制所选图层"else"复制图层")},enabled=vm.canCopyLayers(),modifier=Modifier.testTag("copy-layers"),onClick={more=false;vm.copyLayers()})
                DropdownMenuItem(text={Column{Text("粘贴图层");vm.layerPasteHint()?.let{Text(it,color=Muted,fontSize=12.sp)}}},enabled=vm.canPasteLayers(),modifier=Modifier.testTag("paste-layers"),onClick={more=false;vm.pasteLayers()})
                HorizontalDivider(color=Muted.copy(alpha=.15f))
                if(compact)DropdownMenuItem(text={Text("重做")},enabled=vm.state.canRedo,onClick={more=false;vm.redo()})
                DropdownMenuItem(text={Text(if(vm.currentKey()==null)"添加关键帧" else "删除当前关键帧")},enabled=vm.editable(),onClick={more=false;vm.toggleKey()})
                DropdownMenuItem(text={Text("观察视图")},onClick={more=false;vm.observe(!vm.state.observing)})
            }
        }
        }
    }
    }
}

@Composable internal fun InputDialog(title:String,initial:String,onDismiss:()->Unit,numeric:Boolean=false,onConfirm:(String)->Unit) {
    var value by remember(initial){mutableStateOf(initial)}
    AlertDialog(onDismissRequest=onDismiss,title={Text(title)},
        text={Column {
            OutlinedTextField(value,{value=it},singleLine=true,modifier=Modifier.fillMaxWidth(),
                keyboardOptions=KeyboardOptions(keyboardType=if(numeric)KeyboardType.Decimal else KeyboardType.Text,imeAction=ImeAction.Done),
                keyboardActions=KeyboardActions(onDone={onConfirm(value)}))
            if(numeric)TextButton(onClick={value=if(value.startsWith("-"))value.drop(1) else "-"+value},modifier=Modifier.heightIn(min=48.dp)) {
                Text("切换正负号")
            }
        }},
        confirmButton={TextButton(onClick={onConfirm(value)}){Text("确定")}},
        dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}
