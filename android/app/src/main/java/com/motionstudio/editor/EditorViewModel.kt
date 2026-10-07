package com.motionstudio.editor

import android.app.Application
import android.content.Intent
import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.*
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import android.view.Choreographer
import android.view.Surface
import android.widget.Toast
import androidx.compose.runtime.*
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.*

data class StudioState(
    val project: JSONObject? = null, val sample: JSONObject? = null,
    val canUndo: Boolean = false, val canRedo: Boolean = false,
    val observing: Boolean = false, val error: String? = null,
    val busy: Boolean = false, val saved: Boolean = false,
)
data class ProjectSummary(val directory:String,val name:String,val width:Int,val height:Int,val fps:Int,val modified:Long)
internal fun JSONArray?.objects():List<JSONObject> = this?.let{a->(0 until a.length()).map{a.getJSONObject(it)}}?:emptyList()
internal fun effectTarget(key:String):Pair<Long,String>? {
    if(!key.startsWith("effect:"))return null
    val parts=key.removePrefix("effect:").split(':',limit=2)
    return if(parts.size==2)parts[0].toLongOrNull()?.let{it to parts[1]}else null
}

private fun activeProjectDirectory(app:Application):File {
    val saved=app.getSharedPreferences("motion-studio",0).getString("activeProject","default")?:"default"
    val name=if(saved=="default"||saved.matches(Regex("(?:import|project)-[0-9]+")))saved else "default"
    return File(app.filesDir,"studio/"+name)
}

class EditorViewModel @JvmOverloads constructor(app: Application,projectDirectory:File?=null,initialProjectJson:String="") : AndroidViewModel(app) {
    internal val updates=ReleaseUpdates(app,viewModelScope)
    private val errors=(app as? StudioApplication)?.errors?:ErrorReports(app)
    private var pendingDiagnostic:File?=null
    internal val gestureInertia=GestureInertiaGroup()
    internal val layoutPreferences=app.getSharedPreferences("motion-studio-layout"+
        (if(projectDirectory==null)""else"-"+projectDirectory.canonicalPath.hashCode()),0)
    var state by mutableStateOf(StudioState()); private set
    var expressionTarget by mutableStateOf<JSONObject?>(null); private set
    private var shownRenderError:String?=null
    var frame by mutableDoubleStateOf(0.0); private set
    var compositionId by mutableStateOf("comp-main");private set
    var compositionPath by mutableStateOf(listOf("comp-main"));private set
    var compositionTimeline by mutableStateOf(JSONObject());private set
    @Volatile private var engineComposition="comp-main"
    var playing by mutableStateOf(false); private set
    var selected by mutableLongStateOf(0L)
    var layerSelectionMode by mutableStateOf(false);private set
    var selectedLayerIds by mutableStateOf<Set<Long>>(emptySet());private set
    var property by mutableStateOf("position")
    var panelOpen by mutableStateOf(false)
    var effectsOpen by mutableStateOf(false)
    var vectorOpen by mutableStateOf(false)
    var compositionClipOpen by mutableStateOf(false)
    var vectorTab by mutableStateOf("geometry")
    var vectorDrawMode by mutableStateOf(false)
    var vectorPathId by mutableLongStateOf(1L)
    var vectorNodeId by mutableLongStateOf(0L)
    var vectorHandleMode by mutableStateOf("corner")
    var catalogue by mutableStateOf<JSONObject?>(null); private set
    var importTask by mutableStateOf<JSONObject?>(null); private set
    var mediaNotice by mutableStateOf<String?>(null); private set
    var waveforms by mutableStateOf<Map<Long,JSONObject>>(emptyMap()); private set
    private var pendingMedia:String?=null
    private var preparedRoot:File?=null
    private val mediaGeneration=AtomicLong()
    private val projectGeneration=AtomicLong()
    private val playGeneration=AtomicLong()
    private val audioPlayer=AudioPlayback {message->main.post{pause();fail(message)}}
    var timelineScale by mutableFloatStateOf(1.5f)
    var scaleLinked by mutableStateOf(true)
    var rotationAxis by mutableIntStateOf(2)
    var componentAxis by mutableIntStateOf(0)
    var curveClipboard by mutableStateOf<String?>(null); private set
    private var layerClipboard by mutableStateOf<LayerClipboard?>(null)
    private var layerClipboardBusy by mutableStateOf(false)
    var previewMode by mutableIntStateOf(if(projectDirectory==null)app.getSharedPreferences("motion-studio",0).getInt("previewMode",0).coerceIn(0,3) else 0);private set
    var previewInfo by mutableStateOf<JSONObject?>(null);private set
    var loadFailed by mutableStateOf(false);private set
    var projects by mutableStateOf<List<ProjectSummary>>(emptyList());private set
    var projectsLoading by mutableStateOf(false);private set
    var lastSavedOutput by mutableStateOf<Uri?>(null);private set
    var pendingOutput:File?=null
    var pendingOutputKind:String="png"
    var lastOutputSelection:Uri?=null;private set
    var lastGpuFailure:String?=null;private set
    @Volatile var outputPhase:String="idle";private set
    val isClosed:Boolean get()=closed.get()
    var exporting by mutableStateOf(false); private set
    var exportProgress by mutableFloatStateOf(0f); private set
    private var exporter:VideoExporter?=null
    private val persistProjectSelection=projectDirectory==null
    var root = (projectDirectory?:activeProjectDirectory(app)).apply { mkdirs() }.canonicalFile; private set
    private val workerThread = HandlerThread("motion-render").apply { start() }
    private val worker = Handler(workerThread.looper)
    private val main = Handler(app.mainLooper)
    private class GestureUpdates {
        val pending=LinkedHashMap<String,String>()
        var queued=false
        fun take():List<String> = synchronized(this){pending.values.toList().also{pending.clear()}}
    }
    private var gestureUpdates:GestureUpdates?=null
    private var id = 0L
    private val closed = AtomicBoolean(false)
    internal val pluginEditor=PluginEditorHost(worker,main,{id},{closed.get()},{engineComposition}) {save->
        publish(if(save)NativeBridge.save(id)else NativeBridge.state(id),if(save)true else null)
    }
    private val queued = AtomicBoolean(false)
    private val surfaceReady = AtomicBoolean(false)
    private val surfaceRequest=AtomicLong()
    private val pickRequest=AtomicLong()
    private val dirty = AtomicBoolean(true)
    private val foreground=AtomicBoolean(true)
    private var currentSurface:Surface?=null
    private var surfaceWidth=0
    private var surfaceHeight=0
    private var startNanos = 0L
    private var startFrame = 0.0
    private var nextPreviewNanos=0L
    private var lastPreviewInfoNanos=0L
    // Owned by the render worker. Let an asynchronous video request finish
    // before advancing its target; changing it every tick cancels decoding.
    private var pendingPlaybackFrame:Double?=null
    private var pendingPlaybackGeneration=-1L
    private val thermalMonitor=app.getSystemService(PowerManager::class.java)
    private var thermalStatus=thermalMonitor.currentThermalStatus
    private val thermalListener=PowerManager.OnThermalStatusChangedListener {status->thermalStatus=status;applyPreviewMode()}
    private val tick = object : Choreographer.FrameCallback {
        override fun doFrame(time: Long) {
            if (closed.get()) return
            val p = state.project
            if (playing && p != null) {
                frame = audioPlayer.frame(p.getInt("fps")) ?: playbackFrame(startFrame,startNanos,time,p.getInt("fps"),p.getInt("frames"))
                dirty.set(true)
            }
            val due=!playing||time+100_000>=nextPreviewNanos
            if (due && foreground.get() && dirty.get() && surfaceReady.get() && queued.compareAndSet(false,true)) {
                dirty.set(false)
                val target=frame
                val playback=playing
                val playbackGeneration=playGeneration.get()
                nextPreviewNanos=time+1_000_000_000L/(previewInfo?.optInt("fps",60)?:60)
                val refreshPreview=time-lastPreviewInfoNanos>=1_000_000_000L
                if(refreshPreview)lastPreviewInfoNanos=time
                worker.post {
                    try {
                        if(playback&&playbackGeneration!=playGeneration.get()) {
                            pendingPlaybackFrame=null;dirty.set(true);return@post
                        }
                        if(!playback||pendingPlaybackGeneration!=playbackGeneration)pendingPlaybackFrame=null
                        if(id!=0L && foreground.get() && surfaceReady.get()) {
                            val renderTarget=if(playback)pendingPlaybackFrame?:target else target
                            val rendered=CompositionBridge.render(id,engineComposition,renderTarget)
                            pendingPlaybackGeneration=playbackGeneration
                            pendingPlaybackFrame=if(playback&&!rendered)renderTarget else null
                            if(!rendered) {
                                val envelope=JSONObject(NativeBridge.state(id))
                                val error=envelope.optJSONObject("data")?.optString("renderError","")?.takeIf{it!="null"&&it.isNotBlank()}
                                if(error?.startsWith("expression ")==true)publish(envelope.toString(),repaint=false)
                                else if(error!=null){surfaceReady.set(false);main.post{lastGpuFailure=error};fail("预览暂不可用，请重试预览。工程数据已保留。")}else dirty.set(true)
                            }
                        }
                        if(refreshPreview&&id!=0L)updatePreviewInfo(NativeBridge.previewInfo(id))
                    }
                    catch(e:Throwable) { fail(e.message?:"预览失败") }
                    finally { queued.set(false) }
                }
            }
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    init {
        thermalMonitor.addThermalStatusListener(app.mainExecutor,thermalListener)
        worker.post {
            try {
                id=NativeBridge.create(root.absolutePath,if(File(root,"project.json").exists())"" else initialProjectJson)
                check(id!=0L){"工程无法打开："+NativeBridge.creationError()}
                updatePreviewInfo(NativeBridge.previewMode(id,previewMode,thermalStatus))
                publish(NativeBridge.state(id),root.resolve("project.json").exists())
                readCatalogue()
            } catch(e:Throwable) {main.post{loadFailed=true};fail(e.message?:"原生引擎初始化失败")}
        }
        Choreographer.getInstance().postFrameCallback(tick)
    }
    private fun fail(message:String,surfaceGeneration:Long?=null,cause:Throwable?=null) {main.post {
        if(surfaceGeneration==null||surfaceGeneration==surfaceRequest.get()) {
            state=state.copy(error=compositionErrorMessage(message),busy=false)
            val p=state.project
            val context=JSONObject().put("frame",frame).put("selected_kind",contentKind()).put("property",property)
                .put("playing",playing).put("output_phase",outputPhase.substringBefore(':')).put("preview_mode",previewMode)
            if(p!=null)context.put("composition",JSONObject().put("width",p.optInt("width")).put("height",p.optInt("height"))
                .put("fps",p.optInt("fps")).put("frames",p.optInt("frames")).put("layer_count",p.optJSONArray("layers")?.length()?:0))
            viewModelScope.launch(Dispatchers.IO){errors.record(message,cause,context)}
        }
    }}
    fun prepareErrorReport(onReady:()->Unit) {
        viewModelScope.launch(Dispatchers.IO){try {
            pendingDiagnostic=errors.export();withContext(Dispatchers.Main){onReady()}
        }catch(e:Throwable){fail("错误报告生成失败",cause=e)}}
    }
    fun saveErrorReport(uri:Uri?) {
        val file=pendingDiagnostic?:return;pendingDiagnostic=null
        if(uri==null)return
        viewModelScope.launch(Dispatchers.IO){try {
            val output=getApplication<Application>().contentResolver.openOutputStream(uri,"w")?:error("无法写入所选位置")
            output.use{out->file.inputStream().use{it.copyTo(out)}}
            withContext(Dispatchers.Main){Toast.makeText(getApplication(),"错误报告已保存",Toast.LENGTH_LONG).show()}
        }catch(e:Throwable){fail("错误报告保存失败，请重新选择位置",cause=e)}}
    }
    fun clearError() {state=state.copy(error=null)}
    internal fun showOperationError(message:String){fail(message)}
    private fun updatePreviewInfo(raw:String) {
        val result=JSONObject(raw)
        if(!result.optBoolean("ok")){fail(result.optString("error"));return}
        val info=result.getJSONObject("data")
        main.post{if(!closed.get())previewInfo=info}
    }
    private fun applyPreviewMode() {
        val mode=previewMode;val thermal=thermalStatus
        worker.post{if(id!=0L){updatePreviewInfo(NativeBridge.previewMode(id,mode,thermal));dirty.set(true)}}
    }
    fun choosePreviewMode(mode:Int) {
        require(mode in 0..3);previewMode=mode;nextPreviewNanos=0
        if(persistProjectSelection)getApplication<Application>().getSharedPreferences("motion-studio",0).edit().putInt("previewMode",mode).apply()
        applyPreviewMode()
    }
    fun startProfiling(maxFrames:Int,onStarted:()->Unit) {
        worker.post{try{
            val result=JSONObject(NativeBridge.startProfiling(id,maxFrames));check(result.optBoolean("ok")){result.optString("error")}
            main.post(onStarted)
        }catch(e:Throwable){fail(e.message?:"计时启动失败")}}
    }
    fun stopProfiling(onComplete:(File)->Unit) {
        worker.post{try {
            val result=JSONObject(NativeBridge.stopProfiling(id));check(result.optBoolean("ok")){result.optString("error")}
            val file=File(result.getJSONObject("data").getString("file"));main.post{onComplete(file)}
        }catch(e:Throwable){fail(e.message?:"计时保存失败")}}
    }
    fun completeOutputSelection(uri:Uri?) {
        lastOutputSelection=uri
        if(uri==null){pendingOutput=null;return}
        val file=pendingOutput
        if(file==null){fail("导出源文件已失效，请重新导出");return}
        saveOutput(file,uri)
    }
    fun saveOutput(file:File,uri:Uri) {
        state=state.copy(busy=true);lastSavedOutput=null;outputPhase="queued"
        viewModelScope.launch(Dispatchers.IO) {
            try {
                outputPhase="opening"
                val resolver=getApplication<Application>().contentResolver
                runCatching{resolver.takePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)}
                val output=resolver.openOutputStream(uri,"w")?:error("所选位置无法写入")
                outputPhase="copying"
                output.use{out->file.inputStream().use{it.copyTo(out)}}
                outputPhase="completing"
                withContext(Dispatchers.Main){lastSavedOutput=uri;pendingOutput=null;state=state.copy(busy=false)}
                outputPhase="done"
            }catch(e:Throwable){outputPhase="failed: "+e.javaClass.simpleName+": "+e.message
                fail(if(e is SecurityException)"所选位置未授予写入权限，请重新选择文件夹。导出源文件已保留。" else "保存文件失败："+(e.message?:"请重新选择位置"))}
        }
    }
    private fun publish(raw:String,saved:Boolean?=null,repaint:Boolean=true,surfaceGeneration:Long?=null) {
        try {
            val r=JSONObject(raw)
            if(!r.optBoolean("ok")) {
                val error=r.optString("error","操作失败")
                if(error.startsWith("expression ")&&id!=0L)publish(NativeBridge.state(id),saved,repaint=false,surfaceGeneration=surfaceGeneration)
                else fail(error,surfaceGeneration)
                return
            }
            val d=r.getJSONObject("data")
            engineComposition=d.optString("composition","comp-main")
            if(pluginEditor.session!=null)pluginEditor.refresh()
            main.post {
                if(!closed.get()&&(surfaceGeneration==null||surfaceGeneration==surfaceRequest.get())) {
                    val selectedExisted=selected!=0L&&layer(selected)!=null
                    val savedState=saved?:if(state.sample!=null&&state.sample!!.optLong("revision")!=d.optLong("revision"))false else state.saved
                    val renderError=if(d.isNull("renderError"))null else d.optString("renderError")
                    val nextError=renderError?:state.error.takeUnless{it==shownRenderError}
                    shownRenderError=renderError
                    state=StudioState(d.getJSONObject("project"),d,d.optBoolean("canUndo"),
                        d.optBoolean("canRedo"),d.optBoolean("observing"),
                        nextError,false,savedState)
                    val nextComposition=d.optString("composition","comp-main")
                    val context=d.optJSONObject("composition_context")
                    compositionPath=context?.optJSONArray("path")?.let{a->(0 until a.length()).map{a.getString(it)}}?:listOf(nextComposition)
                    if(nextComposition!=compositionId) {
                        compositionId=nextComposition;closeWorkspace();waveforms=emptyMap();property="position";frame=d.optDouble("frame",0.0)
                        val selection=context?.optJSONArray("selection")?.let{a->(0 until a.length()).map{a.getLong(it)}}.orEmpty()
                        selected=selection.firstOrNull()?:0;selectedLayerIds=selection.filter{it!=0L}.toSet();layerSelectionMode=selectedLayerIds.size>1
                        compositionTimeline=context?.optJSONObject("timeline")?:JSONObject()
                        timelineScale=compositionTimeline.optDouble("zoom",1.5).toFloat().coerceIn(.4f,12f)
                    }
                    selectedLayerIds=selectedLayerIds.intersect(state.project?.optJSONArray("layers").objects().map{it.getLong("id")}.toSet())
                    d.optJSONObject("edit_result")?.takeIf{it.optString("op")=="split_layer_clip"}?.let {
                        if(selected==it.optLong("left_object"))selected=it.getLong("right_object")
                    }
                    d.optJSONArray("edit_results").objects().filter{it.optString("op")=="composition"}.forEach{edit->edit.optJSONObject("result")?.optJSONArray("selection")?.let{a->selected=if(a.length()>0)a.getLong(0)else 0L;finishLayerSelection()}}
                    if(selectedExisted&&selected!=0L&&layer(selected)==null){selected=0L;panelOpen=false;expressionTarget=null;pluginEditor.close()}
                    if(d.has("root")) {
                        val nextRoot=File(d.getString("root"))
                        if(nextRoot!=root){finishLayerSelection();closeWorkspace();frame=d.optDouble("frame",0.0);selected=0L;property="position";compositionTimeline=context?.optJSONObject("timeline")?:JSONObject();timelineScale=compositionTimeline.optDouble("zoom",1.5).toFloat().coerceIn(.4f,12f)}
                        root=nextRoot
                        if(persistProjectSelection)getApplication<Application>().getSharedPreferences("motion-studio",0)
                            .edit().putString("activeProject",root.name).apply()
                    }
                    if(preparedRoot!=root)prepareMediaCaches()
                    // Seek updates the UI immediately. Older worker replies must
                    // not pull the playhead back while the user is scrubbing.
                    if(repaint)dirty.set(true)
                }
            }
        } catch(e:Throwable){fail(e.message?:"状态读取失败")}
    }
    private fun invoke(save:Boolean=false,repaint:Boolean=true,onComplete:(()->Unit)?=null,operation:()->String) {
        if(closed.get())return
        worker.post {
            if(id==0L||closed.get())return@post
            try {
                val result=operation()
                JSONObject(result).optJSONObject("data")?.optString("composition")?.takeIf{it.isNotBlank()}?.let{engineComposition=it}
                if(save&&JSONObject(result).optBoolean("ok")) {
                    val saved=JSONObject(NativeBridge.save(id))
                    val edits=JSONObject(result).optJSONObject("data")
                    for(key in listOf("edit_result","edit_results"))edits?.opt(key)?.let{saved.optJSONObject("data")?.put(key,it)}
                    publish(saved.toString(),true)
                }
                else publish(result,repaint=repaint)
            } catch(e:Throwable){fail(e.message?:"操作失败",cause=e)}
            finally {if(onComplete!=null)main.post{if(!closed.get())onComplete()}}
        }
    }
    /** Only absolute assignments can replace earlier updates in one gesture. */
    private fun gestureKey(command:JSONObject):String? {
        val op=command.optString("op")
        if(op=="effect") {
            val action=command.getJSONObject("action")
            if(action.optString("kind") !in listOf("set","curve","set_curve_object"))return null
            return "effect:${command.getLong("object")}:${action.getLong("effect")}:${action.getString("param")}:${action.optInt("frame")}:${action.getString("kind")}"
        }
        if(op=="vector") {
            val action=command.getJSONObject("action")
            return "vector:${command.getLong("object")}:${action.optString("action")}:${action.optLong("path")}:${action.optLong("node")}:${action.optString("parameter")}"
        }
        if(op !in listOf("set_vector","set_scalar","set_component","set_audio"))return null
        return "$op:${command.getLong("object")}:${command.optString("property")}:${command.optString("axis")}:${command.optInt("frame")}:${command.has("volume")}:${command.has("muted")}"
    }
    private fun queueGesture(commands:List<JSONObject>):Boolean {
        val updates=gestureUpdates?:return false
        val keys=commands.map{gestureKey(it)?:return false}
        val schedule=synchronized(updates) {
            commands.forEachIndexed{i,command->updates.pending[keys[i]]=command.toString()}
            if(updates.queued)false else {updates.queued=true;true}
        }
        if(schedule)worker.post {
            val pending=synchronized(updates){updates.queued=false;updates.take()}
            if(pending.isNotEmpty()&&id!=0L&&!closed.get())try {
                publish(NativeBridge.command(id,JSONArray(pending.map{JSONObject(it)}).toString()))
            }catch(e:Throwable){fail(e.message?:"参数更新失败")}
        }
        return true
    }
    fun attach(surface:Surface,width:Int,height:Int) {
        currentSurface=surface;surfaceWidth=width;surfaceHeight=height
        val request=surfaceRequest.incrementAndGet()
        surfaceReady.set(false)
        worker.post {
            if(id==0L||closed.get()||request!=surfaceRequest.get()||!surface.isValid)return@post
            try {
                val result=NativeBridge.surface(id,surface,width,height)
                if(request==surfaceRequest.get()&&surface.isValid) {
                    val success=JSONObject(result).optBoolean("ok")
                    surfaceReady.set(success)
                    if(success)main.post{loadFailed=false;lastGpuFailure=null}
                    publish(result,surfaceGeneration=request)
                }
            }catch(error:Throwable){if(request==surfaceRequest.get())fail(error.message?:"预览初始化失败",request)}
        }
    }
    fun detach() {
        val request=surfaceRequest.incrementAndGet()
        currentSurface=null
        surfaceReady.set(false)
        worker.post {if(id!=0L&&request==surfaceRequest.get())NativeBridge.surface(id,null,0,0)}
    }
    fun pause() {
        playGeneration.incrementAndGet();audioPlayer.stop()
        if(playing) {playing=false;val f=frame;invoke {CompositionBridge.seek(id,engineComposition,f)}}
    }
    fun suspendPreview(){pause();foreground.set(false)}
    fun resumePreview(){foreground.set(true);dirty.set(true)}
    fun refreshDiagnostics(repaint:Boolean=false){invoke(repaint=repaint){NativeBridge.state(id)}}
    fun retryPreview(){currentSurface?.takeIf{it.isValid}?.let{clearError();attach(it,surfaceWidth,surfaceHeight)}}
    fun injectGraphicsFault(kind:Int) {
        require(getApplication<Application>().applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE!=0){"Diagnostics require a debug application"}
        require(state.sample?.optBoolean("diagnosticsEnabled")==true){"This native build excludes GPU diagnostic injection"}
        worker.post {
            val result=JSONObject(NativeBridge.injectGraphicsFault(id,kind))
            if(!result.optBoolean("ok"))fail(result.optString("error"))else dirty.set(true)
        }
    }
    private fun attachRecoveredSession():String {
        return currentSurface?.takeIf{it.isValid}?.let{surface->
            val result=NativeBridge.surface(id,surface,surfaceWidth,surfaceHeight)
            val envelope=JSONObject(result);surfaceReady.set(envelope.optBoolean("ok"))
            check(envelope.optBoolean("ok")){envelope.optString("error","预览初始化失败")};result
        }?:NativeBridge.state(id)
    }
    fun retryOpen() {
        if(id!=0L){clearError();retryPreview();return}
        state=state.copy(busy=true,error=null)
        worker.post {
            try {
                id=NativeBridge.create(root.absolutePath,"");check(id!=0L){"工程无法打开："+NativeBridge.creationError()}
                val result=attachRecoveredSession();main.post{loadFailed=false};publish(result,true)
            }catch(e:Throwable){fail(e.message?:"工程重试失败")}
        }
    }
    fun refreshProjects() {
        val active=root;val parent=active.parentFile?:return
        projectsLoading=true
        viewModelScope.launch(Dispatchers.IO) {
            val library=parent.canonicalFile
            val items=parent.listFiles().orEmpty().filter{it.isDirectory&&(it.name==active.name||it.name=="default"||it.name.matches(Regex("(?:project|import)-[0-9]+")))}
                .mapNotNull{folder->runCatching {
                    if(folder.canonicalFile.parentFile!=library)return@runCatching null
                    val file=File(folder,"project.json");if(!file.isFile||file.length()>16*1024*1024)return@runCatching null
                    val json=JSONObject(file.readText())
                    ProjectSummary(folder.name,json.optString("name",folder.name),json.optInt("width"),json.optInt("height"),json.optInt("fps"),file.lastModified())
                }.getOrNull()}.sortedByDescending{it.modified}
            withContext(Dispatchers.Main){projects=items;projectsLoading=false}
        }
    }
    fun openProject(directory:String) {
        finishLayerSelection()
        projectGeneration.incrementAndGet()
        closeWorkspace();pause();cancelMediaImport();state=state.copy(busy=true,error=null)
        worker.post {
            try {
                if(id==0L) {
                    val parent=root.parentFile!!.canonicalFile
                    val file=File(parent,directory).canonicalFile
                    check(file.parentFile==parent){"工程目录无效"}
                    id=NativeBridge.create(file.absolutePath,"");check(id!=0L){"工程无法打开："+NativeBridge.creationError()}
                    val result=attachRecoveredSession();main.post{loadFailed=false};publish(result,true)
                }else {
                    val result=NativeBridge.openProject(id,directory)
                    if(JSONObject(result).optBoolean("ok"))main.post{loadFailed=false}
                    publish(result,true)
                }
            }catch(e:Throwable){fail(e.message?:"打开工程失败")}
        }
    }
    fun togglePlay() {
        gestureInertia.stop()
        if(pluginEditor.gesture)return
        if(playing)pause() else if(state.project!=null) {
            val p=state.project!!
            val hasSound=state.sample?.optBoolean("has_audio")==true
            if(!hasSound){startFrame=frame;startNanos=System.nanoTime();playing=true;return}
            val at=frame;val ticket=playGeneration.incrementAndGet()
            worker.post {try {
                val frozen=nativeData(CompositionBridge.freezeAudio(id,engineComposition))
                val handle=frozen.getLong("handle")
                if(ticket!=playGeneration.get()){MediaBridge.releaseFrozenAudio(handle);return@post}
                audioPlayer.start(handle,(at*48000/p.getInt("fps")).toLong(),frozen.getLong("total_frames")) {
                    main.post {if(ticket==playGeneration.get()){startFrame=at;startNanos=System.nanoTime();playing=true}}
                }
            }catch(e:Throwable){fail(e.message?:"声音播放失败")}}
        }
    }
    fun seek(value:Double) {
        if(pluginEditor.gesture)return
        if(!value.isFinite()){fail("帧位置无效");return}
        pause();val p=state.project?:return
        frame=value.coerceIn(0.0,p.getInt("frames")-1.0)
        val f=frame;invoke {CompositionBridge.seek(id,engineComposition,f)}
    }
    fun step(delta:Int)=seek(floor(frame)+delta)
    fun select(objectId:Long,openEditor:Boolean=true) {
        gestureInertia.stop();finishLayerSelection()
        pause()
        if(selected!=objectId) {
            pluginEditor.close()
            expressionTarget=null
            effectsOpen=false
            vectorOpen=false
            compositionClipOpen=false
            selected=objectId
            property=if(objectId==0L&&state.project?.optJSONObject("camera")?.optString("mode")=="orbit")"radius" else "position"
        }
        if(openEditor)panelOpen=true
    }
    fun openProperty(key:String) {gestureInertia.stop();pluginEditor.close();expressionTarget=null;effectsOpen=false;vectorOpen=false;compositionClipOpen=false;
        pause()
        property=if(selected==0L&&key=="position"&&state.project?.optJSONObject("camera")?.optString("mode")=="orbit")"radius" else key
        panelOpen=true
    }
    fun hasCamera()=state.project?.optJSONObject("camera")?.optBoolean("created",true)==true
    internal fun compositionQuery(op:String,target:String=compositionId,fields:JSONObject=JSONObject(),onResult:(JSONObject)->Unit) {
        val sourceRoot=root;val sourceComposition=compositionId
        worker.post {try {
            val result=nativeData(CompositionBridge.request(id,JSONObject(fields.toString()).put("version",1).put("composition",target).put("op",op).toString()))
            main.post{if(!closed.get()&&root==sourceRoot&&compositionId==sourceComposition)onResult(result)}
        }catch(e:Throwable){fail(compositionErrorMessage(e.message?:"合成操作失败"),cause=e)}}
    }
    internal fun compositionAction(action:JSONObject,save:Boolean=true) = edit(JSONObject().put("op","composition").put("action",action),save)
    internal fun canPrecompose():Boolean {
        val layers=if(layerSelectionMode)selectedLayers()else listOfNotNull(layer(selected))
        if(layers.isEmpty()||layers.any{it.optBoolean("locked")||it.optBoolean("three_d")})return false
        val ids=layers.map{it.getLong("id")}.toSet()
        val indices=state.project?.optJSONArray("layers").objects().mapIndexedNotNull{index,l->index.takeIf{l.getLong("id") in ids}}
        return indices.isNotEmpty()&&indices.last()-indices.first()+1==indices.size
    }
    internal fun precompose(name:String) {
        if(!canPrecompose()){fail("请选择连续且未锁定的二维图层");return}
        compositionAction(JSONObject().put("kind","precompose").put("objects",JSONArray(if(layerSelectionMode)selectedLayerIds.toList()else listOf(selected))).put("name",name.ifBlank{"预合成"}).put("range","composition"))
        finishLayerSelection();closeWorkspace()
    }
    internal fun openComposition(target:String,path:List<String> = listOf(target)) {
        if(importTask!=null||exporting){fail("请先完成或取消导入、导出，再切换合成");return}
        if(state.busy)return
        gestureInertia.stop();if(gestureUpdates!=null)endGesture();closeWorkspace();pause()
        val previous=compositionId
        val selection=if(layerSelectionMode)selectedLayerIds.toList()else listOf(selected).filter{it!=0L||hasCamera()}
        val timeline=JSONObject(compositionTimeline.toString()).put("zoom",timelineScale)
        state=state.copy(busy=true)
        invoke {
            nativeData(CompositionBridge.request(id,JSONObject().put("version",1).put("composition",previous).put("op","context").put("selection",JSONArray(selection)).put("timeline",timeline).toString()))
            CompositionBridge.request(id,JSONObject().put("version",1).put("composition",target).put("op","open").put("path",JSONArray(path)).toString())
        }
    }
    internal fun openSelectedComposition() {
        val target=layer(selected)?.optJSONObject("content")?.optJSONObject("clip")?.optString("composition")?:return
        openComposition(target,compositionPath+target)
    }
    internal fun applyCompositionSettings(settings:JSONObject,revision:Long) {
        val target=compositionId
        invoke(true){CompositionBridge.request(id,JSONObject().put("version",1).put("composition",target).put("op","settings_apply").put("expected_revision",revision).put("settings",settings).toString())}
    }
    fun editable():Boolean=!state.busy&&(if(selected==0L)hasCamera()else layer(selected)?.optBoolean("locked")==false)
    fun edit(command:JSONObject,save:Boolean=true) {
        errors.operation("edit:"+command.optString("op"))
        if(pluginEditor.gesture)return
        if(command.optString("op") in listOf("remove","flags","camera_mode","set_layer_3d"))pluginEditor.close()
        pause();val routed=(routeVectorCommand(routeEffectCommand(command))?:return).put("composition",compositionId)
        if(!save&&queueGesture(listOf(routed)))return
        invoke(save){NativeBridge.command(id,routed.toString())}
    }
    fun editBatch(commands:JSONArray,save:Boolean=true) {
        errors.operation("batch:"+commands.objects().map{it.optString("op")}.distinct().joinToString(","))
        if(pluginEditor.gesture)return
        pause();val routed=commands.objects().map{(routeVectorCommand(routeEffectCommand(it))?:return).put("composition",compositionId)}
        if(!save&&queueGesture(routed))return
        invoke(save){NativeBridge.command(id,JSONArray(routed).toString())}
    }
    fun undo() {errors.operation("undo");gestureInertia.stop();if(pluginEditor.gesture)return;pause();invoke(true){CompositionBridge.history(id,engineComposition,0)}}
    fun redo() {errors.operation("redo");gestureInertia.stop();if(pluginEditor.gesture)return;pause();invoke(true){CompositionBridge.history(id,engineComposition,1)}}
    fun beginGesture() {gestureInertia.stop();pluginEditor.close();pause();gestureUpdates=GestureUpdates();invoke{CompositionBridge.history(id,engineComposition,2)}}
    fun endGesture(onComplete:()->Unit={}) {
        val pending=gestureUpdates?.take().orEmpty();gestureUpdates=null
        invoke(true,onComplete=onComplete) {
            if(pending.isNotEmpty()) {
                val result=NativeBridge.command(id,JSONArray(pending.map{JSONObject(it)}).toString())
                if(!JSONObject(result).optBoolean("ok")){CompositionBridge.history(id,engineComposition,4);return@invoke result}
            }
            CompositionBridge.history(id,engineComposition,3)
        }
    }
    fun cancelGesture() {gestureUpdates?.take();gestureUpdates=null;invoke{CompositionBridge.history(id,engineComposition,4)}}
    fun moveLayer(dx:Float,dy:Float,width:Int,height:Int) {
        val objectId=selected
        if(objectId==0L)return
        property="position"
        invoke{CompositionBridge.drag(id,engineComposition,objectId,dx.toDouble(),dy.toDouble(),width,height)}
    }
    fun save() {invoke(true){NativeBridge.state(id)}}
    fun layer(id:Long):JSONObject? {
        val list=state.project?.optJSONArray("layers")?:return null
        return (0 until list.length()).map{list.getJSONObject(it)}.firstOrNull{it.getLong("id")==id}
    }
    fun timelineLayer(objectId:Long):JSONObject?=state.sample?.optJSONArray("timeline_layers")?.let{a->
        (0 until a.length()).map{a.getJSONObject(it)}.firstOrNull{it.getLong("object")==objectId}}
    fun propertyTrack(objectId:Long=selected,key:String=property):JSONObject? {
        if(key.startsWith("vector:"))return vectorTrackRaw(objectId,key)?.let{raw->
            JSONObject(raw.toString()).apply{val offset=timelineLayer(objectId)?.optInt("offset_frame")?:0
                optJSONArray("keys").objects().forEach{it.put("frame",it.getLong("frame")+offset)}}
        }
        effectTarget(key)?.let{(instance,param)->
            val data=effectParam(objectId,instance,param)?:return null
            val track=data.optJSONObject("curve")?:data.optJSONObject("track")?:return null
            val result=JSONObject(track.toString())
            val offset=timelineLayer(objectId)?.optInt("offset_frame")?:0
            result.optJSONArray("keys")?.objects()?.forEach{it.put("frame",it.getLong("frame")+offset)}
            return result
        }
        val p=state.project?:return null
        return if(objectId==0L)state.sample?.optJSONObject("timeline_camera")?.optJSONObject(key)
            ?:p.getJSONObject("camera").optJSONObject(key)
        else timelineLayer(objectId)?.optJSONObject("properties")?.optJSONObject(key)
            ?:layer(objectId)?.getJSONObject("transform")?.optJSONObject(key)
    }
    fun isSeparated(objectId:Long=selected,key:String=property)=propertyTrack(objectId,key)?.has("axes")==true
    fun activeAxis()=if(property=="rotation")rotationAxis else componentAxis
    fun axisName(axis:Int=activeAxis())=listOf("x","y","z")[axis]
    fun chooseAxis(axis:Int){pause();if(property=="rotation")rotationAxis=axis else componentAxis=axis}
    fun threeD(objectId:Long=selected)=objectId==0L||layer(objectId)?.optBoolean("three_d",true)==true
    fun pickLayer(x:Float,y:Float,width:Float,height:Float,onPicked:(Long?)->Unit) {
        val p=state.project?:return
        val fit=min(width/p.getInt("width"),height/p.getInt("height"))
        if(fit<=0)return
        val px=(x-(width-p.getInt("width")*fit)/2)/fit
        val py=(y-(height-p.getInt("height")*fit)/2)/fit
        val at=frame;val projectRoot=root;val sourceComposition=compositionId;val request=pickRequest.incrementAndGet()
        worker.post {
            if(id==0L||closed.get())return@post
            try {
                val seek=JSONObject(CompositionBridge.seek(id,engineComposition,at));check(seek.optBoolean("ok")){seek.optString("error")}
                val result=JSONObject(GeometryBridge.hitCandidates(id,px.toDouble(),py.toDouble()))
                check(result.optBoolean("ok")){result.optString("error")}
                val candidates=result.getJSONObject("data").getJSONArray("candidates")
                val picked=if(candidates.length()==0)null else candidates.getJSONObject(0).getLong("id")
                main.post{if(!closed.get()&&request==pickRequest.get()&&root==projectRoot&&compositionId==sourceComposition&&frame==at)onPicked(picked)}
            }catch(e:Throwable){main.post{if(!closed.get()&&request==pickRequest.get()&&root==projectRoot&&compositionId==sourceComposition&&frame==at){fail(e.message?:"图层点选失败");onPicked(null)}}}
        }
    }
    fun visibleAxes():List<Int> = if(threeD())listOf(0,1,2)else if(property=="rotation")listOf(2)else listOf(0,1)
    fun canSeparate():Boolean {
        val capability=state.sample?.optJSONObject("capabilities")?.optJSONObject("separate_dimensions")?:return false
        val names=capability.optJSONArray(if(selected==0L)"camera_properties"else"layer_properties")?:return false
        return capability.optBoolean("supported")&&!isSeparated()&&(0 until names.length()).any{names.getString(it)==property}
    }
    fun separateDimensions(){if(canSeparate()&&editable()) {
        if(property=="scale")scaleLinked=false
        edit(JSONObject().put("op","separate_dimensions").put("object",selected).put("property",property))
    }}
    fun setThreeD(enabled:Boolean){if(selected!=0L&&editable())edit(JSONObject().put("op","set_layer_3d").put("object",selected).put("enabled",enabled))}
    fun track():JSONObject?=propertyTrack()?.let{if(isSeparated())it.optJSONObject("axes")?.optJSONObject(axisName())else it}
    private fun channelCommand(op:String,objectId:Long=selected,key:String=property,axis:Int?=if(isSeparated(objectId,key))activeAxis()else null)=
        JSONObject().put("op",op).put("object",objectId).put("property",key).apply{axis?.let{put("axis",axisName(it))}}
    fun sampleValue():Any? {
        return sampleValueFor(selected,property)
    }
    fun sampleValueFor(objectId:Long,key:String):Any? {
        if(key.startsWith("vector:"))return vectorValue(objectId,key)
        effectTarget(key)?.let{(instance,param)->
            val saved=effectParam(objectId,instance,param)
            if(saved?.optString("kind")=="curve") {
                val track=propertyTrack(objectId,key)?:return null
                val keys=track.optJSONArray("keys").objects()
                val value=keys.lastOrNull{it.getDouble("frame")<=frame}?.getJSONObject("value")?:keys.firstOrNull()?.getJSONObject("value")?:track.getJSONObject("value")
                val result=JSONObject(value.toString())
                state.sample?.optJSONArray("sampledEffects").objects().firstOrNull{it.getLong("layer")==objectId&&it.getLong("instance")==instance}?.optJSONArray("curve_lut")?.let{result.put("sampled_lut",it)}
                return result
            }
            return state.sample?.optJSONArray("sampledEffects").objects().firstOrNull{it.getLong("layer")==objectId&&it.getLong("instance")==instance}
                ?.optJSONObject("values")?.optJSONArray(param) ?: effectParam(objectId,instance,param)?.optJSONObject("track")?.optJSONArray("value")
        }
        val s=state.sample?:return null
        return if(objectId==0L)s.optJSONObject("sampledCamera")?.opt(key)
        else s.optJSONArray("sampledLayers")?.let{a->
            (0 until a.length()).map{a.getJSONObject(it)}.firstOrNull{it.getLong("id")==objectId}?.opt(key)}
    }
    fun setValue(value:Any,save:Boolean=true) {
        setPropertyValue(selected,property,floor(frame).toInt(),value,save)
    }
    fun setPropertyValue(objectId:Long,key:String,at:Int,value:Any,save:Boolean=true,editedAxes:List<Int>?=null) {
        if(value is JSONArray&&isSeparated(objectId,key)) {
            val commands=JSONArray()
            (editedAxes?:listOf(0,1,2)).forEach{axis->commands.put(channelCommand("set_component",objectId,key,axis).put("frame",at).put("value",value.getDouble(axis)))}
            editBatch(commands,save);return
        }
        edit(JSONObject().put("op",if(value is JSONArray)"set_vector" else "set_scalar")
            .put("object",objectId).put("property",key).put("frame",at).put("value",value),save)
    }
    fun animate() {
        val keys=track()?.optJSONArray("keys")?:return
        edit(channelCommand("animate")
            .put("frame",floor(frame).toInt()).put("enabled",keys.length()==0))
    }
    fun addKey() {
        val sample=sampleValue()?:return
        val value=if(isSeparated()&&sample is JSONArray)sample.getDouble(activeAxis())else sample
        val t=track()?:return
        if(property.startsWith("vector:")) {
            edit(channelCommand(if(value is JSONArray)"set_vector"else"set_scalar").put("frame",floor(frame).toInt()).put("value",value).put("animated",true));return
        }
        val commands=JSONArray()
        if(t.getJSONArray("keys").length()==0)commands.put(channelCommand("animate").put("frame",floor(frame).toInt()).put("enabled",true))
        commands.put(channelCommand(if(isSeparated())"set_component"else if(value is JSONArray)"set_vector"else"set_scalar")
            .put("frame",floor(frame).toInt()).put("value",value))
        editBatch(commands)
    }
    fun keys():List<JSONObject> = track()?.optJSONArray("keys")?.let{a->(0 until a.length()).map{a.getJSONObject(it)}.filter{it.getLong("frame") in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()}}?:emptyList()
    fun currentKey():JSONObject?=keys().firstOrNull{it.getInt("frame")==floor(frame).toInt()}
    fun toggleKey(){currentKey()?.let{deleteKey(it.getInt("frame"))}?:addKey()}
    fun jumpKey(next:Boolean) {
        val frames=keys().map{it.getInt("frame")}.filter{it in 0 until (state.project?.optInt("frames")?:0)}
        val target=if(next)frames.firstOrNull{it>frame}else frames.lastOrNull{it<frame}
        target?.let{seek(it.toDouble())}
    }
    fun deleteKey(key:Int)=edit(channelCommand("delete_key").put("frame",key))
    fun moveKey(from:Int,to:Int)=edit(channelCommand("move_key").put("from",from).put("to",to))
    fun moveKeyFor(objectId:Long,key:String,from:Int,to:Int,axis:Int?=if(isSeparated(objectId,key))activeAxis()else null)=edit(channelCommand("move_key",objectId,key,axis).put("from",from).put("to",to))
    fun copyKey(from:Int,to:Int)=edit(channelCommand("copy_key").put("from",from).put("to",to))
    fun copyKeyFor(objectId:Long,key:String,from:Int,to:Int,axis:Int?)=edit(channelCommand("copy_key",objectId,key,axis).put("from",from).put("to",to))
    fun deleteKeyFor(objectId:Long,key:String,frame:Int,axis:Int?)=edit(channelCommand("delete_key",objectId,key,axis).put("frame",frame))
    fun easingSegment():Pair<JSONObject,JSONObject>?=keys().zipWithNext().firstOrNull{(a,b)->frame>=a.getInt("frame")&&frame<b.getInt("frame")}
    fun ease(mode:String) {
        val key=easingSegment()?.first?:return
        edit(channelCommand("ease").put("frame",key.getInt("frame")).put("ease",mode))
    }
    fun easingDefinition():JSONObject?=easingSegment()?.first?.let{key->
        JSONObject().put("ease",key.optString("ease","linear")).apply{key.optJSONObject("curve")?.let{put("curve",JSONObject(it.toString()))}}
    }
    fun setCurve(easing:JSONObject,save:Boolean=true) {
        val key=easingSegment()?.first?:return
        if(!editable())return
        edit(channelCommand("curve")
            .put("frame",key.getInt("frame")).put("easing",JSONObject(easing.toString())),save)
    }
    fun copyCurve() {
        val easing=easingDefinition()?:return
        val payload=JSONObject().put("format","motionstudio.curve").put("version",1).put("easing",easing).toString()
        curveClipboard=payload
        getApplication<Application>().getSystemService(ClipboardManager::class.java)
            .setPrimaryClip(ClipData.newPlainText("Motion Studio 曲线",payload))
    }
    fun refreshCurveClipboard() {
        val clipboard=getApplication<Application>().getSystemService(ClipboardManager::class.java)
        if(clipboard.primaryClipDescription?.label?.toString()!="Motion Studio 曲线")return
        val text=clipboard.primaryClip?.getItemAt(0)?.text?.toString()?:return
        if(text.length>4096)return
        val payload=runCatching{JSONObject(text)}.getOrNull()?:return
        if(payload.optString("format")=="motionstudio.curve"&&payload.optInt("version")==1&&payload.optJSONObject("easing")!=null)curveClipboard=text
    }
    fun pasteCurve() {
        refreshCurveClipboard()
        val payload=curveClipboard?.let{runCatching{JSONObject(it)}.getOrNull()}?:return
        payload.optJSONObject("easing")?.let{setCurve(it)}
    }
    fun observe(enabled:Boolean,azimuth:Double=0.0,elevation:Double=0.0) {
        pause();invoke{NativeBridge.observe(id,enabled,azimuth,elevation)}
    }
    fun navigate(dx:Float,dy:Float,zoom:Float,multi:Boolean,width:Int,height:Int) {
        pause();invoke{NativeBridge.navigate(id,dx.toDouble(),dy.toDouble(),zoom.toDouble(),multi,width,height)}
    }
    fun recordOrbit(azimuth:Double,elevation:Double,at:Int) {
        editBatch(JSONArray().put(JSONObject().put("op","set_scalar").put("object",0).put("property","azimuth").put("frame",at).put("value",azimuth))
            .put(JSONObject().put("op","set_scalar").put("object",0).put("property","elevation").put("frame",at).put("value",elevation.coerceIn(-89.0,89.0))),false)
    }
    fun anchor(x:Double,y:Double) {if(selected!=0L)edit(JSONObject().put("op","anchor").put("object",selected).put("anchor",JSONArray(listOf(x,y))))}
    fun focusCameraOnSelection() {
        if(selected==0L)return
        val target=sampleValueFor(selected,"position") as? JSONArray?:return
        val camera=state.project?.getJSONObject("camera")?:return
        val at=floor(frame).toInt()
        val commands=JSONArray().put(JSONObject().put("op","set_vector").put("object",0).put("property","target").put("frame",at).put("value",target))
        if(camera.getString("mode")=="orbit") {
            val eye=state.sample?.getJSONObject("sampledCamera")?.getJSONArray("position")?:return
            val dx=eye.getDouble(0)-target.getDouble(0);val dy=eye.getDouble(1)-target.getDouble(1);val dz=eye.getDouble(2)-target.getDouble(2)
            val radius=sqrt(dx*dx+dy*dy+dz*dz)
            if(radius<1){fail("目标点距离摄影机太近");return}
            val values=listOf("radius" to radius,"azimuth" to Math.toDegrees(atan2(dx,-dz)),"elevation" to Math.toDegrees(asin((-dy/radius).coerceIn(-1.0,1.0))).coerceIn(-89.0,89.0))
            values.forEach{(key,value)->commands.put(JSONObject().put("op","set_scalar").put("object",0).put("property",key).put("frame",at).put("value",value))}
        }
        editBatch(commands);selected=0;property="target";panelOpen=true
    }
    fun view(kind:Int) {pause();invoke{NativeBridge.view(id,kind)}}
    fun dolly(amount:Float)=edit(JSONObject().put("op","dolly").put("frame",floor(frame).toInt()).put("amount",amount),false)
    fun pan(x:Float,y:Float)=edit(JSONObject().put("op","pan").put("frame",floor(frame).toInt()).put("x",x).put("y",y),false)
    fun cameraMode(orbit:Boolean)=edit(JSONObject().put("op","camera_mode").put("mode",if(orbit)"orbit" else "position"))
    fun flags(id:Long,visible:Boolean,locked:Boolean)=edit(JSONObject().put("op","flags").put("object",id).put("visible",visible).put("locked",locked))
    fun rename(name:String) {if(selected!=0L)edit(JSONObject().put("op","rename").put("object",selected).put("name",name))}
    fun duplicate() {if(selected!=0L)edit(JSONObject().put("op","duplicate").put("object",selected))}
    fun canCopyLayers()=!state.busy&&importTask==null&&!layerClipboardBusy&&
        (if(layerSelectionMode)selectedLayers().isNotEmpty()else selected!=0L&&layer(selected)!=null)
    fun canPasteLayers()=!state.busy&&importTask==null&&!layerClipboardBusy&&
        layerClipboard?.let{clip->clip.root==root.canonicalFile&&clip.composition==compositionId&&state.project?.let(clip::available)==true}==true
    fun layerPasteHint():String?=layerClipboard?.let{clip->
        when {clip.root!=root.canonicalFile->"在原工程内粘贴";clip.composition!=compositionId->"请返回原合成粘贴";state.project?.let(clip::available)==false->"素材或父级已变化，请重新复制";else->null}
    }
    fun copyLayers() {
        if(!canCopyLayers())return
        gestureInertia.stop();pause()
        val sourceRoot=root.canonicalFile;val sourceComposition=compositionId;val objects=if(layerSelectionMode)selectedLayerIds else setOf(selected)
        layerClipboardBusy=true
        invoke(repaint=false,onComplete={
            layerClipboardBusy=false
            if(root.canonicalFile==sourceRoot&&compositionId==sourceComposition)state.project?.let{project->LayerClipboard.capture(sourceRoot,project,objects)}?.let{clip->
                layerClipboard=clip
                Toast.makeText(getApplication(),"已复制 ${clip.size} 个图层",Toast.LENGTH_SHORT).show()
            }
        }){NativeBridge.state(id)}
    }
    fun pasteLayers() {
        if(!canPasteLayers())return
        gestureInertia.stop();pluginEditor.close();pause()
        val sourceRoot=root.canonicalFile;val clip=layerClipboard?:return;val at=floor(frame).toInt()
        var pastedObjects=emptyList<Long>()
        layerClipboardBusy=true
        invoke(true,onComplete={
            layerClipboardBusy=false
            if(root.canonicalFile==sourceRoot&&pastedObjects.isNotEmpty()&&pastedObjects.all{layer(it)!=null}) {
                closeWorkspace();selected=pastedObjects.last();property=if(contentKind()=="audio")"audio"else"position"
                layerSelectionMode=pastedObjects.size>1;selectedLayerIds=if(layerSelectionMode)pastedObjects.toSet()else emptySet()
            }
        }){
            val current=nativeData(NativeBridge.state(id))
            check(File(current.getString("root")).canonicalFile==sourceRoot&&current.optString("composition","comp-main")==clip.composition){"工程已切换，请重新复制"}
            val paste=clip.plan(current.getJSONObject("project"),at)?:error("素材或父级已变化，请重新复制")
            val result=NativeBridge.command(id,JSONArray(paste.commands.objects().map{it.put("composition",engineComposition)}).toString())
            if(JSONObject(result).optBoolean("ok"))pastedObjects=paste.objects
            result
        }
    }
    fun startLayerSelection(objectId:Long=selected) {
        closeWorkspace();pause();layerSelectionMode=true
        selectedLayerIds=if(objectId!=0L&&layer(objectId)!=null)setOf(objectId)else emptySet()
    }
    fun finishLayerSelection(){layerSelectionMode=false;selectedLayerIds=emptySet()}
    fun toggleLayerSelection(objectId:Long) {
        if(!layerSelectionMode||objectId==0L||layer(objectId)==null)return
        selectedLayerIds=if(objectId in selectedLayerIds)selectedLayerIds-objectId else selectedLayerIds+objectId
    }
    fun selectAllLayers(){selectedLayerIds=state.project?.optJSONArray("layers").objects().map{it.getLong("id")}.toSet()}
    fun clearLayerSelection(){selectedLayerIds=emptySet()}
    fun selectedLayers()=state.project?.optJSONArray("layers").objects().filter{it.getLong("id") in selectedLayerIds}
    fun selectionEditable()=selectedLayers().let{it.isNotEmpty()&&it.none{layer->layer.getBoolean("locked")}}
    fun selectedFlags(visible:Boolean?=null,locked:Boolean?=null) {
        val commands=selectedLayers().map{layer->JSONObject().put("op","flags").put("object",layer.getLong("id"))
            .put("visible",visible?:layer.getBoolean("visible")).put("locked",locked?:layer.getBoolean("locked"))}
        if(commands.isNotEmpty())editBatch(JSONArray(commands))
    }
    fun duplicateSelectedLayers(){if(selectionEditable())editBatch(JSONArray(selectedLayers().map{JSONObject().put("op","duplicate").put("object",it.getLong("id"))}))}
    fun deleteSelectedLayers() {
        if(!selectionEditable())return
        editBatch(JSONArray(selectedLayers().map{JSONObject().put("op","remove").put("object",it.getLong("id")).put("frame",floor(frame).toInt())}))
        finishLayerSelection()
    }
    fun selectedClipDeltaRange():IntRange? {
        if(!selectionEditable())return null
        val clips=selectedLayers().map{timelineLayer(it.getLong("id"))?:return null}
        val frames=state.project?.optInt("frames")?:return null
        val first=clips.maxOf{-it.getInt("in_frame")};val last=clips.minOf{frames-it.getInt("out_frame")}
        return (first..last).takeUnless{it.isEmpty()}
    }
    fun moveSelectedClips(delta:Int):Boolean {
        val range=selectedClipDeltaRange()?:return false
        if(delta !in range)return false
        if(delta!=0)editBatch(JSONArray(selectedLayers().map{layer->JSONObject().put("op","move_layer_clip").put("object",layer.getLong("id"))
            .put("in_frame",timelineLayer(layer.getLong("id"))!!.getInt("in_frame")+delta)}))
        return true
    }
    fun deleteLayer() {if(selected!=0L||hasCamera()){edit(JSONObject().put("op","remove").put("object",selected).put("frame",floor(frame).toInt()));selected=0L;property="position";panelOpen=false;effectsOpen=false}}
    fun reorder(delta:Int) {
        val p=state.project?:return;val a=p.getJSONArray("layers")
        val index=(0 until a.length()).firstOrNull{a.getJSONObject(it).getLong("id")==selected}?:return
        edit(JSONObject().put("op","reorder").put("object",selected).put("index",(index+delta).coerceIn(0,a.length()-1)))
    }
    fun reorderTo(objectId:Long,index:Int)=edit(JSONObject().put("op","reorder").put("object",objectId).put("index",index))
    fun moveClip(objectId:Long,start:Int,save:Boolean=true)=edit(JSONObject().put("op","move_layer_clip").put("object",objectId).put("in_frame",start),save)
    fun trimClip(objectId:Long,start:Int,end:Int,save:Boolean=true)=edit(JSONObject().put("op","trim_layer_clip").put("object",objectId).put("in_frame",start).put("out_frame",end),save)
    fun canSplitClip():Boolean {
        if(selected==0L||layerSelectionMode||!editable()||state.busy||importTask!=null)return false
        val clip=timelineLayer(selected)?:return false
        return floor(frame).toInt() in clip.getInt("in_frame")+1 until clip.getInt("out_frame")
    }
    fun splitClip(){gestureInertia.stop();pause();if(canSplitClip())edit(JSONObject().put("op","split_layer_clip").put("object",selected).put("frame",floor(frame).toInt()))}
    private fun nextId(a:JSONArray):Long=(0 until a.length()).maxOfOrNull{a.getJSONObject(it).getLong("id")}?.plus(1)?:1
    private fun nextAssetId(project:JSONObject):Long {
        val highest=listOf("assets","audio_assets","video_assets")
            .flatMap{project.optJSONArray(it).objects()}.maxOfOrNull{it.getLong("id")}?:0L
        check(highest<Long.MAX_VALUE){"素材 ID 已用尽"}
        return highest+1
    }
    private fun channel(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun newLayer(name:String,content:JSONObject,width:Float,height:Float):JSONObject {
        val p=state.project!!
        return JSONObject().put("id",nextId(p.getJSONArray("layers"))).put("name",name).put("content",content)
            .put("size",JSONArray(listOf(width,height))).put("visible",true).put("locked",false).put("three_d",false)
            .put("transform",JSONObject().put("position",channel(JSONArray(listOf(p.getInt("width")/2f,p.getInt("height")/2f,0))))
                .put("rotation",channel(JSONArray(listOf(0,0,0)))).put("scale",channel(JSONArray(listOf(100,100,100))))
                .put("opacity",channel(1)).put("anchor",JSONArray(listOf(0.5,0.5))))
    }
    fun addRectangle() {
        val p=state.project?:return
        val l=newLayer("矩形",JSONObject().put("kind","solid").put("color",JSONArray(listOf(0.43,0.68,0.91,1))),
            p.getInt("width")*.5f,p.getInt("height")*.2f)
        edit(JSONObject().put("op","add").put("layer",l));selected=l.getLong("id");property="position";panelOpen=true
    }
    fun addCamera(){edit(JSONObject().put("op","create_camera"));selected=0L;property="position";panelOpen=true}
    fun addNull() {
        val l=newLayer("空对象",JSONObject().put("kind","null"),100f,100f)
        edit(JSONObject().put("op","add").put("layer",l));selected=l.getLong("id");property="position";panelOpen=true
    }
    fun setParent(parent:Long?){edit(JSONObject().put("op","parent").put("object",selected).put("parent",parent?:JSONObject.NULL).put("frame",floor(frame).toInt()))}
    fun importImage(uri:Uri) {
        val p=state.project?:return;pause();state=state.copy(busy=true)
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val source=ImageDecoder.createSource(getApplication<Application>().contentResolver,uri)
                val bitmap=ImageDecoder.decodeBitmap(source){decoder,info,_->
                    check(info.size.width.toLong()*info.size.height*4<=64L*1024*1024){"图片超出单图内存预算，请先缩小原图"}
                    decoder.allocator=ImageDecoder.ALLOCATOR_SOFTWARE
                }
                val file=File(root,"assets/"+UUID.randomUUID()+".png").apply{parentFile!!.mkdirs()}
                file.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
                val assetId=nextAssetId(p)
                val a=JSONObject().put("id",assetId).put("path","assets/"+file.name).put("width",bitmap.width).put("height",bitmap.height)
                val l=newLayer("图片",JSONObject().put("kind","image").put("asset",assetId),bitmap.width.toFloat(),bitmap.height.toFloat())
                bitmap.recycle()
                withContext(Dispatchers.Main) {
                    editBatch(JSONArray().put(JSONObject().put("op","register_asset").put("asset",a)).put(JSONObject().put("op","add").put("layer",l)))
                    selected=l.getLong("id");property="position";panelOpen=true
                }
            } catch(e:Throwable){fail(e.message?:"图片导入失败")}
        }
    }
    fun addText(text:String) {
        val p=state.project?:return;pause();state=state.copy(busy=true)
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val paint=Paint(Paint.ANTI_ALIAS_FLAG).apply{textSize=96f;color=Color.WHITE;typeface=Typeface.DEFAULT_BOLD}
                val w=ceil(paint.measureText(text)+32).toInt().coerceAtLeast(32)
                check(w<=4096){"文字太长，请缩短后添加"}
                val bitmap=Bitmap.createBitmap(w,160,Bitmap.Config.ARGB_8888)
                Canvas(bitmap).drawText(text,16f,112f,paint)
                val file=File(root,"assets/"+UUID.randomUUID()+".png").apply{parentFile!!.mkdirs()}
                file.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
                val aid=nextAssetId(p)
                val a=JSONObject().put("id",aid).put("path","assets/"+file.name).put("width",w).put("height",160)
                val content=JSONObject().put("kind","text").put("text",text).put("font","sans-bold")
                    .put("color",JSONArray(listOf(1,1,1,1))).put("raster_asset",aid)
                val l=newLayer("文字",content,w.toFloat(),160f);bitmap.recycle()
                withContext(Dispatchers.Main) {
                    editBatch(JSONArray().put(JSONObject().put("op","register_asset").put("asset",a)).put(JSONObject().put("op","add").put("layer",l)))
                    selected=l.getLong("id");property="position";panelOpen=true
                }
            } catch(e:Throwable){fail(e.message?:"文字添加失败")}
        }
    }
    fun output(png:Boolean,callback:(File)->Unit) {
        pause();state=state.copy(busy=true)
        worker.post {
            try {
                val r=JSONObject(if(png)CompositionBridge.capture(id,engineComposition)else NativeBridge.pack(id))
                if(!r.optBoolean("ok")){fail(r.optString("error"));return@post}
                val file=File(r.getJSONObject("data").getString("path"))
                main.post{state=state.copy(busy=false);callback(file)}
            } catch(e:Throwable){fail(e.message?:"输出失败")}
        }
    }
    fun frozenProject(callback:(String)->Unit) {
        pause()
        worker.post {
            val r=JSONObject(NativeBridge.state(id))
            if(r.optBoolean("ok")) {
                val json=r.getJSONObject("data").getJSONObject("project").toString()
                main.post{callback(json)}
            } else fail(r.optString("error"))
        }
    }
    fun newProject(width:Int,height:Int,fps:Int=30,name:String="新建工程",frames:Int=fps*6) {
        if(width !in 1..8192||height !in 1..8192||fps !in 1..240||frames !in 1..36000){fail("合成尺寸、帧率或时长无效");return}
        finishLayerSelection()
        projectGeneration.incrementAndGet()
        closeWorkspace();pause();cancelMediaImport();state=state.copy(busy=true,error=null)
        val target=JSONArray(listOf(width/2f,height/2f,0))
        val distance=height/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("created",false).put("mode","position").put("position",channel(JSONArray(listOf(width/2f,height/2f,-distance))))
            .put("target",channel(target)).put("roll",channel(0)).put("fov",channel(45))
            .put("radius",channel(distance)).put("azimuth",channel(0)).put("elevation",channel(0))
        val project=JSONObject().put("version",3).put("name",name.trim().ifBlank{"新建工程"}).put("width",width).put("height",height)
            .put("fps",fps).put("frames",frames).put("background",JSONArray(listOf(.05,.06,.09,1)))
            .put("camera",camera).put("assets",JSONArray()).put("layers",JSONArray())
        worker.post {
            try {
                if(id==0L) {
                    val destination=File(root.parentFile,"project-"+System.nanoTime()).apply{mkdirs()}
                    id=NativeBridge.create(destination.absolutePath,project.toString());check(id!=0L){NativeBridge.creationError()}
                    val saved=JSONObject(NativeBridge.save(id));check(saved.optBoolean("ok")){saved.optString("error")}
                    val result=attachRecoveredSession();main.post{loadFailed=false};publish(result,true)
                }else {
                    val result=NativeBridge.newProject(id,project.toString())
                    if(JSONObject(result).optBoolean("ok"))main.post{loadFailed=false;selected=0L;property="position";panelOpen=false;effectsOpen=false}
                    publish(result,true)
                }
            }catch(e:Throwable){fail(e.message?:"新建工程失败")}
        }
    }
    fun importProject(uri:Uri) {
        finishLayerSelection()
        projectGeneration.incrementAndGet()
        closeWorkspace();pause();cancelMediaImport();state=state.copy(busy=true)
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val file=File(root.parentFile,"incoming-"+UUID.randomUUID()+".motion")
                try {
                    getApplication<Application>().contentResolver.openInputStream(uri)?.use{input->file.outputStream().use{out->
                        val buffer=ByteArray(32*1024);var total=0L
                        while(true){val read=input.read(buffer);if(read<0)break;total+=read;check(total<=512L*1024*1024){"工程包超过 512 MiB"};out.write(buffer,0,read)}
                    }}?:error("无法打开工程文件")
                }catch(error:Throwable){file.delete();throw error}
                worker.post {
                    val recovering=id==0L
                    var temporary:File?=null
                    try {
                        if(recovering) {
                            temporary=File(root.parentFile,"project-"+System.nanoTime()).apply{mkdirs()}
                            id=NativeBridge.create(temporary!!.absolutePath,"");check(id!=0L){NativeBridge.creationError()}
                        }
                        val result=NativeBridge.importProject(id,file.absolutePath)
                        val envelope=JSONObject(result)
                        check(envelope.optBoolean("ok")){envelope.optString("error","工程导入失败")}
                        val saved=NativeBridge.save(id);check(JSONObject(saved).optBoolean("ok")){JSONObject(saved).optString("error")}
                        val stateResult=if(recovering)attachRecoveredSession()else saved
                        main.post{loadFailed=false};publish(stateResult,true)
                    }catch(e:Throwable) {
                        if(recovering&&id!=0L){NativeBridge.destroy(id);id=0L}
                        fail(e.message?:"工程导入失败")
                    }finally {file.delete();temporary?.delete()}
                }
            } catch(e:Throwable){fail(e.message?:"工程导入失败")}
        }
    }
    fun exportVideo(callback:(File)->Unit) {
        if(exporting)return
        frozenProject {json->
            val task=VideoExporter(root,json)
            exporter=task;exporting=true;exportProgress=0f
            viewModelScope.launch(Dispatchers.IO) {
                try {
                    val file=task.run{done,total->main.post{exportProgress=done.toFloat()/total}}
                    withContext(Dispatchers.Main){exporting=false;exporter=null;callback(file)}
                } catch(e:Throwable) {
                    withContext(Dispatchers.Main) {
                        exporting=false;exporter=null
                        if(!task.cancelled.get())state=state.copy(error=e.message?:"视频导出失败")
                    }
                }
            }
        }
    }
    fun contentKind(objectId:Long=selected)=layer(objectId)?.optJSONObject("content")?.optString("kind")
    fun audioClip(objectId:Long=selected):JSONObject? {
        timelineLayer(objectId)?.optJSONObject("audio")?.let{return it}
        val content=layer(objectId)?.optJSONObject("content")?:return null
        if(content.optString("kind")=="audio")return content.optJSONObject("audio")
        val clip=content.optJSONObject("video")?:return null
        val asset=state.project?.optJSONArray("video_assets").objects().firstOrNull{it.getLong("id")==clip.getLong("asset")}?:return null
        if(asset.isNull("audio_asset"))return null
        return JSONObject(clip.toString()).put("asset",asset.getLong("audio_asset"))
    }
    fun setAudio(objectId:Long,volume:Double?=null,muted:Boolean?=null,save:Boolean=true) {
        edit(JSONObject().put("op","set_audio").put("object",objectId).apply{volume?.let{put("volume",it.coerceIn(0.0,2.0))};muted?.let{put("muted",it)}},save)
    }
    fun effectInstance(objectId:Long,instance:Long)=layer(objectId)?.optJSONArray("effects").objects().firstOrNull{it.getLong("id")==instance}
    fun effectParam(objectId:Long,instance:Long,param:String)=effectInstance(objectId,instance)?.optJSONObject("params")?.optJSONObject(param)
    fun effectDefinition(instance:JSONObject):JSONObject?=catalogue?.optJSONArray("packages").objects().firstOrNull{pkg->
        val m=pkg.getJSONObject("manifest");m.getString("id")==instance.getString("plugin")&&m.getString("version")==instance.getString("version")&&pkg.getString("hash")==instance.getString("hash")
    }?.getJSONObject("manifest")?.getJSONArray("effects").objects()?.firstOrNull{it.getString("id")==instance.getString("effect")}
    fun effectAction(objectId:Long,instance:Long,kind:String,extras:JSONObject=JSONObject(),save:Boolean=true) {
        val action=JSONObject(extras.toString()).put("kind",kind).put("effect",instance)
        edit(JSONObject().put("op","effect").put("object",objectId).put("action",action),save)
    }
    private fun routeEffectCommand(command:JSONObject):JSONObject {
        val target=effectTarget(command.optString("property"))?:return command
        val action=JSONObject(command.toString())
        val op=action.getString("op");val objectId=action.getLong("object")
        action.remove("op");action.remove("object");action.remove("property");action.remove("axis")
        action.put("kind",when(op){"set_vector","set_scalar"->if(action.opt("value") is JSONObject)"set_curve_object"else"set";"ease"->"curve";else->op})
        if(op=="ease"){action.put("easing",JSONObject().put("ease",action.getString("ease")));action.remove("ease")}
        return JSONObject().put("op","effect").put("object",objectId).put("action",action.put("effect",target.first).put("param",target.second))
    }
    private fun readCatalogue() {
        val result=nativeData(CompositionBridge.plugin(id,engineComposition,JSONObject().put("op","catalogue")))
        main.post{if(!closed.get())catalogue=result}
    }
    fun refreshCatalogue(){worker.post{try{readCatalogue()}catch(e:Throwable){fail(e.message?:"效果目录读取失败")}}}
    fun pluginOperation(request:JSONObject,save:Boolean=false) {
        pluginEditor.close();pause();worker.post {try {
            val raw=CompositionBridge.plugin(id,engineComposition,request)
            nativeData(raw)
            if(save)publish(NativeBridge.save(id),true)else publish(raw)
            readCatalogue()
        }catch(e:Throwable){fail(e.message?:"效果操作失败")}}
    }
    fun closeWorkspace(){gestureInertia.stop();expressionTarget=null;pluginEditor.close();effectsOpen=false;vectorOpen=false;compositionClipOpen=false;panelOpen=false}
    internal fun openCompositionClip(){closeWorkspace();pause();compositionClipOpen=true;panelOpen=true}
    fun openEffects(){gestureInertia.stop();pluginEditor.close();expressionTarget=null;vectorOpen=false;compositionClipOpen=false;pause();panelOpen=true;property="position";refreshCatalogue();effectsOpen=true}
    fun openPluginEditor(instance:Long){pause();expressionTarget=null;pluginEditor.open(selected,instance)}
    fun expressionTargetForCurrent(axis:Int?=null):JSONObject? {
        if(state.sample?.optJSONObject("capabilities")?.optJSONObject("property_expressions")?.optBoolean("supported")!=true)return null
        val effect=effectTarget(property)
        if(effect!=null) {
            val param=effectParam(selected,effect.first,effect.second)?:return null
            if(!param.optBoolean("implemented",true)||!param.optBoolean("animatable",true)||param.optString("kind") !in listOf("float","vec2","vec3","color"))return null
            return JSONObject().put("kind","effect").put("object",selected).put("effect",effect.first).put("param",effect.second)
        }
        if(property !in listOf("position","rotation","scale","opacity","target","fov","roll","radius","azimuth","elevation"))return null
        return JSONObject().put("kind","property").put("object",selected).put("property",property).apply{
            axis?.takeIf{sampleValue() is JSONArray&&it in 0..2}?.let{put("axis",listOf("x","y","z")[it])}
        }
    }
    fun expressionFor(target:JSONObject):JSONObject?=state.project?.optJSONArray("expressions").objects().firstOrNull{expression->
        val saved=expression.getJSONObject("target")
        listOf("kind","object","property","axis","effect","param").all{saved.optString(it,"")==target.optString(it,"")}
    }
    fun openExpression(target:JSONObject) {pluginEditor.close();pause();expressionTarget=JSONObject(target.toString());panelOpen=true}
    fun closeExpression(){expressionTarget=null}
    fun saveExpression(target:JSONObject,source:String,enabled:Boolean,seed:Long,callback:(String?)->Unit) {
        val profile=state.sample?.optJSONObject("capabilities")?.optJSONObject("property_expressions")?.optString("profile")?:return
        expressionCommand(JSONObject().put("op","set_expression").put("frame",floor(frame).toInt())
            .put("expression",JSONObject().put("target",target).put("source",source).put("enabled",enabled).put("seed",seed).put("profile",profile)),callback)
    }
    fun removeExpression(target:JSONObject,callback:(String?)->Unit)=expressionCommand(JSONObject().put("op","remove_expression").put("target",target),callback)
    private fun expressionCommand(command:JSONObject,callback:(String?)->Unit) {
        pause();val request=command.toString();val requestRoot=root;val generation=projectGeneration.get()
        worker.post {
            val error=runCatching {
                check(id!=0L&&!closed.get()){"工程已关闭"}
                check(root==requestRoot&&generation==projectGeneration.get()){"工程已切换"}
                nativeData(NativeBridge.command(id,JSONObject(request).put("composition",engineComposition).toString()))
                publish(NativeBridge.save(id),true)
            }.exceptionOrNull()?.message
            main.post{if(!closed.get()&&root==requestRoot&&generation==projectGeneration.get())callback(error)}
        }
    }
    fun chooseEffectParam(instance:Long,param:String){pause();property="effect:$instance:$param"}
    fun installPlugin(uri:Uri) {
        pause();state=state.copy(busy=true);val sourceRoot=root
        viewModelScope.launch(Dispatchers.IO) {
            val file=File(getApplication<Application>().cacheDir,"incoming-${UUID.randomUUID()}.msfx")
            try {
                getApplication<Application>().contentResolver.openInputStream(uri)?.use{input->file.outputStream().use{out->
                    val chunk=ByteArray(65536);var total=0L
                    while(true){val n=input.read(chunk);if(n<0)break;total+=n;check(total<=16L*1024*1024){"效果包超过 16 MiB"};out.write(chunk,0,n)}
                }}?:error("无法打开效果包")
                worker.post {try {
                    check(root==sourceRoot&&!closed.get()){"工程已切换，请重新安装"}
                    publish(CompositionBridge.plugin(id,engineComposition,JSONObject().put("op","install").put("path",file.absolutePath)))
                    readCatalogue()
                }catch(e:Throwable){fail(e.message?:"效果包安装失败")}finally{file.delete()}}
            }catch(e:Throwable){file.delete();fail(e.message?:"效果包安装失败")}
        }
    }
    fun importMedia(uri:Uri,kind:String,withAudio:Boolean=true) {
        if(pendingMedia!=null||state.project==null)return
        pause();val ticket=mediaGeneration.incrementAndGet();val request="import-${UUID.randomUUID()}"
        val at=floor(frame).toInt();val sourceRoot=root
        importTask=JSONObject().put("state","running").put("phase","opening").put("progress",0).put("kind",kind)
        worker.post {try {
            check(root==sourceRoot){"工程已切换"};pendingMedia=request
            val query=JSONObject().put("op","import_media").put("kind",kind).put("request_id",request).put("uri",uri.toString()).put("at_frame",at)
                .put("name",if(kind=="audio")"音频"else"视频")
            if(kind=="video")query.put("with_audio",withAudio)
            nativeData(MediaBridge.request(id,getApplication<Application>(),query.put("composition",engineComposition).toString()))
            pollMedia(request,ticket)
        }catch(e:Throwable){releaseMedia(request);main.post{importTask=null};fail(e.message?:"媒体导入失败")}}
    }
    private fun releaseMedia(request:String) {
        runCatching{MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","release_media_task").put("request_id",request).toString())}
        if(pendingMedia==request)pendingMedia=null
    }
    private fun pollMedia(request:String,ticket:Long) {
        if(closed.get()||ticket!=mediaGeneration.get())return
        try {
            val task=nativeData(MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","media_status").put("request_id",request).toString()))
            main.post{if(ticket==mediaGeneration.get())importTask=task}
            when(task.getString("state")) {
                "ready"->{
                    val done=nativeData(MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","finish_media_import").put("request_id",request).toString()))
                    val snapshot=done.getJSONObject("state")
                    publish(JSONObject().put("ok",true).put("data",snapshot).toString(),true)
                    val result=done.getJSONObject("task").getJSONObject("edit_result")
                    main.post{
                        effectsOpen=false;selected=result.getLong("object");property=if(result.getString("kind")=="audio")"audio"else"position";panelOpen=true;waveforms=emptyMap()
                        importTask=null;if(result.optBoolean("truncated_to_composition"))mediaNotice="素材已导入，片段尾部已裁至合成结束。"
                    }
                    releaseMedia(request)
                }
                "failed","cancelled"->{releaseMedia(request);main.post{importTask=null};if(task.getString("state")=="failed")fail(task.optString("error","媒体导入失败"))}
                else->worker.postDelayed({pollMedia(request,ticket)},150)
            }
        }catch(e:Throwable){releaseMedia(request);main.post{importTask=null};fail(e.message?:"媒体导入失败")}
    }
    fun cancelMediaImport() {
        mediaGeneration.incrementAndGet();importTask=null
        worker.post{pendingMedia?.let{request->
            runCatching{MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","cancel_media_import").put("request_id",request).toString())}
            releaseMedia(request)
        }}
    }
    fun clearMediaNotice(){mediaNotice=null}
    fun requestWaveform(objectId:Long,firstBucket:Int=0,count:Int=4096) {
        val clip=audioClip(objectId)?:return
        val asset=clip.getLong("asset");val previous=waveforms[objectId]
        if(previous?.optInt("first_bucket")==firstBucket&&previous.optInt("requested_count")==count)return
        val sourceRoot=root;val sourceComposition=compositionId
        worker.post {try {
            val data=nativeData(MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","audio_waveform").put("asset",asset).put("first_bucket",firstBucket.coerceAtLeast(0)).put("count",count.coerceIn(1,4096)).toString())).put("requested_count",count)
            main.post{if(root==sourceRoot&&compositionId==sourceComposition)waveforms=waveforms+(objectId to data)}
        }catch(e:Throwable){fail(e.message?:"波形读取失败")}}
    }
    fun prepareMediaCaches(force:Boolean=false) {
        if(!force&&preparedRoot==root)return
        if(importTask!=null)return
        val sourceRoot=root;preparedRoot=sourceRoot;waveforms=emptyMap()
        val p=state.project?:return
        val work=p.optJSONArray("audio_assets").objects().map{"prepare_audio" to it.getLong("id")}+
            p.optJSONArray("video_assets").objects().map{"prepare_video" to it.getLong("id")}
        if(work.isEmpty())return
        val ticket=mediaGeneration.incrementAndGet()
        fun next(index:Int) {
            if(index==work.size||ticket!=mediaGeneration.get()||closed.get()){main.post{if(ticket==mediaGeneration.get())importTask=null};return}
            val request="cache-${UUID.randomUUID()}"
            worker.post {try {
                check(sourceRoot==root){"工程已切换"};pendingMedia=request
                nativeData(MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op",work[index].first).put("asset",work[index].second).put("request_id",request).toString()))
                fun poll() {
                    if(ticket!=mediaGeneration.get()||closed.get())return
                    try {
                        val status=nativeData(MediaBridge.request(id,null,JSONObject().put("composition",engineComposition).put("op","media_status").put("request_id",request).toString()))
                        main.post{if(ticket==mediaGeneration.get())importTask=status}
                        when(status.getString("state")) {
                            "succeeded"->{releaseMedia(request);next(index+1)}
                            "failed","cancelled"->{releaseMedia(request);main.post{importTask=null};if(status.getString("state")=="failed")fail(status.optString("error","媒体缓存恢复失败"))}
                            else->worker.postDelayed(::poll,150)
                        }
                    }catch(e:Throwable){releaseMedia(request);main.post{importTask=null};fail(e.message?:"媒体缓存恢复失败")}
                }
                poll()
            }catch(e:Throwable){releaseMedia(request);main.post{importTask=null};fail(e.message?:"媒体缓存恢复失败")}}
        }
        importTask=JSONObject().put("phase","opening").put("state","running").put("progress",0)
        next(0)
    }
    fun cancelExport() {exporter?.cancelled?.set(true)}
    override fun onCleared() {
        gestureInertia.stop()
        pluginEditor.close()
        closed.set(true);playing=false
        audioPlayer.close();mediaGeneration.incrementAndGet()
        exporter?.cancelled?.set(true)
        Choreographer.getInstance().removeFrameCallback(tick)
        thermalMonitor.removeThermalStatusListener(thermalListener)
        worker.post{if(id!=0L)NativeBridge.destroy(id);workerThread.quitSafely()}
    }
}
