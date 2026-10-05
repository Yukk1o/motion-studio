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

private fun activeProjectDirectory(app:Application):File {
    val saved=app.getSharedPreferences("motion-studio",0).getString("activeProject","default")?:"default"
    val name=if(saved=="default"||saved.matches(Regex("(?:import|project)-[0-9]+")))saved else "default"
    return File(app.filesDir,"studio/"+name)
}

class EditorViewModel @JvmOverloads constructor(app: Application,projectDirectory:File?=null,initialProjectJson:String="") : AndroidViewModel(app) {
    var state by mutableStateOf(StudioState()); private set
    var frame by mutableDoubleStateOf(0.0); private set
    var playing by mutableStateOf(false); private set
    var selected by mutableLongStateOf(0L)
    var property by mutableStateOf("position")
    var panelOpen by mutableStateOf(false)
    var timelineScale by mutableFloatStateOf(1.5f)
    var scaleLinked by mutableStateOf(true)
    var curveClipboard by mutableStateOf<String?>(null); private set
    var previewMode by mutableIntStateOf(if(projectDirectory==null)app.getSharedPreferences("motion-studio",0).getInt("previewMode",0).coerceIn(0,3) else 0);private set
    var previewInfo by mutableStateOf<JSONObject?>(null);private set
    var loadFailed by mutableStateOf(false);private set
    var projects by mutableStateOf<List<ProjectSummary>>(emptyList());private set
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
    private var id = 0L
    private val closed = AtomicBoolean(false)
    private val queued = AtomicBoolean(false)
    private val surfaceReady = AtomicBoolean(false)
    private val surfaceRequest=AtomicLong()
    private val dirty = AtomicBoolean(true)
    private val foreground=AtomicBoolean(true)
    private var currentSurface:Surface?=null
    private var surfaceWidth=0
    private var surfaceHeight=0
    private var startNanos = 0L
    private var startFrame = 0.0
    private var nextPreviewNanos=0L
    private var lastPreviewInfoNanos=0L
    private val thermalMonitor=app.getSystemService(PowerManager::class.java)
    private var thermalStatus=thermalMonitor.currentThermalStatus
    private val thermalListener=PowerManager.OnThermalStatusChangedListener {status->thermalStatus=status;applyPreviewMode()}
    private val tick = object : Choreographer.FrameCallback {
        override fun doFrame(time: Long) {
            if (closed.get()) return
            val p = state.project
            if (playing && p != null) {
                frame = playbackFrame(startFrame,startNanos,time,p.getInt("fps"),p.getInt("frames"))
                dirty.set(true)
            }
            val due=!playing||time+100_000>=nextPreviewNanos
            if (due && foreground.get() && dirty.get() && surfaceReady.get() && queued.compareAndSet(false,true)) {
                dirty.set(false)
                val target=frame
                nextPreviewNanos=time+1_000_000_000L/(previewInfo?.optInt("fps",60)?:60)
                val refreshPreview=time-lastPreviewInfoNanos>=1_000_000_000L
                if(refreshPreview)lastPreviewInfoNanos=time
                worker.post {
                    try {
                        if(id!=0L && foreground.get() && surfaceReady.get() && !NativeBridge.render(id,target)) {
                        val envelope=JSONObject(NativeBridge.state(id))
                        val error=envelope.optJSONObject("data")?.optString("renderError","")?.takeIf{it!="null"&&it.isNotBlank()}
                        if(error!=null){surfaceReady.set(false);main.post{lastGpuFailure=error};fail("预览暂不可用，请重试预览。工程数据已保留。")}else dirty.set(true)
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
            } catch(e:Throwable) {main.post{loadFailed=true};fail(e.message?:"原生引擎初始化失败")}
        }
        Choreographer.getInstance().postFrameCallback(tick)
    }
    private fun fail(message:String,surfaceGeneration:Long?=null) {main.post {
        if(surfaceGeneration==null||surfaceGeneration==surfaceRequest.get())state=state.copy(error=message,busy=false)
    }}
    fun clearError() {state=state.copy(error=null)}
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
            if(!r.optBoolean("ok")) {fail(r.optString("error","操作失败"),surfaceGeneration);return}
            val d=r.getJSONObject("data")
            main.post {
                if(!closed.get()&&(surfaceGeneration==null||surfaceGeneration==surfaceRequest.get())) {
                    val savedState=saved?:if(state.sample!=null&&state.sample!!.optLong("revision")!=d.optLong("revision"))false else state.saved
                    state=StudioState(d.getJSONObject("project"),d,d.optBoolean("canUndo"),
                        d.optBoolean("canRedo"),d.optBoolean("observing"),
                        if(d.isNull("renderError"))state.error else d.optString("renderError"),false,savedState)
                    if(d.has("root")) {
                        val nextRoot=File(d.getString("root"))
                        if(nextRoot!=root){frame=d.optDouble("frame",0.0);selected=0L;property="position";panelOpen=false}
                        root=nextRoot
                        if(persistProjectSelection)getApplication<Application>().getSharedPreferences("motion-studio",0)
                            .edit().putString("activeProject",root.name).apply()
                    }
                    // Seek updates the UI immediately. Older worker replies must
                    // not pull the playhead back while the user is scrubbing.
                    if(repaint)dirty.set(true)
                }
            }
        } catch(e:Throwable){fail(e.message?:"状态读取失败")}
    }
    private fun invoke(save:Boolean=false,repaint:Boolean=true,operation:()->String) {
        if(closed.get())return
        worker.post {
            if(id==0L||closed.get())return@post
            try {
                val result=operation()
                if(save&&JSONObject(result).optBoolean("ok"))publish(NativeBridge.save(id),true)
                else publish(result,repaint=repaint)
            } catch(e:Throwable){fail(e.message?:"操作失败")}
        }
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
        if(playing) {playing=false;val f=frame;invoke {NativeBridge.seek(id,f)}}
    }
    fun suspendPreview(){pause();foreground.set(false)}
    fun resumePreview(){foreground.set(true);dirty.set(true)}
    fun refreshDiagnostics(){invoke(repaint=false){NativeBridge.state(id)}}
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
        viewModelScope.launch(Dispatchers.IO) {
            val library=parent.canonicalFile
            val items=parent.listFiles().orEmpty().filter{it.isDirectory&&(it.name==active.name||it.name=="default"||it.name.matches(Regex("(?:project|import)-[0-9]+")))}
                .mapNotNull{folder->runCatching {
                    if(folder.canonicalFile.parentFile!=library)return@runCatching null
                    val file=File(folder,"project.json");if(!file.isFile||file.length()>16*1024*1024)return@runCatching null
                    val json=JSONObject(file.readText())
                    ProjectSummary(folder.name,json.optString("name",folder.name),json.optInt("width"),json.optInt("height"),json.optInt("fps"),file.lastModified())
                }.getOrNull()}.sortedByDescending{it.modified}
            withContext(Dispatchers.Main){projects=items}
        }
    }
    fun openProject(directory:String) {
        pause();state=state.copy(busy=true,error=null)
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
        if(playing)pause() else if(state.project!=null) {
            startFrame=frame;startNanos=System.nanoTime();playing=true
        }
    }
    fun seek(value:Double) {
        if(!value.isFinite()){fail("帧位置无效");return}
        pause();val p=state.project?:return
        frame=value.coerceIn(0.0,p.getInt("frames")-1.0)
        val f=frame;invoke {NativeBridge.seek(id,f)}
    }
    fun step(delta:Int)=seek(floor(frame)+delta)
    fun select(objectId:Long,openEditor:Boolean=true) {
        pause()
        if(selected!=objectId) {
            selected=objectId
            property=if(objectId==0L&&state.project?.optJSONObject("camera")?.optString("mode")=="orbit")"radius" else "position"
        }
        if(openEditor)panelOpen=true
    }
    fun openProperty(key:String) {
        pause()
        property=if(selected==0L&&key=="position"&&state.project?.optJSONObject("camera")?.optString("mode")=="orbit")"radius" else key
        panelOpen=true
    }
    fun hasCamera()=state.project?.optJSONObject("camera")?.optBoolean("created",true)==true
    fun editable():Boolean=if(selected==0L)hasCamera()else layer(selected)?.optBoolean("locked")==false
    fun edit(command:JSONObject,save:Boolean=true) {
        pause();invoke(save){NativeBridge.command(id,command.toString())}
    }
    fun editBatch(commands:JSONArray,save:Boolean=true) {pause();invoke(save){NativeBridge.command(id,commands.toString())}}
    fun undo() {pause();invoke(true){NativeBridge.history(id,0)}}
    fun redo() {pause();invoke(true){NativeBridge.history(id,1)}}
    fun beginGesture() {pause();invoke{NativeBridge.history(id,2)}}
    fun endGesture() {invoke(true){NativeBridge.history(id,3)}}
    fun cancelGesture() {invoke{NativeBridge.history(id,4)}}
    fun moveLayer(dx:Float,dy:Float,width:Int,height:Int) {
        val objectId=selected
        if(objectId==0L)return
        property="position"
        invoke{NativeBridge.drag(id,objectId,dx.toDouble(),dy.toDouble(),width,height)}
    }
    fun save() {invoke(true){NativeBridge.state(id)}}
    fun layer(id:Long):JSONObject? {
        val list=state.project?.optJSONArray("layers")?:return null
        return (0 until list.length()).map{list.getJSONObject(it)}.firstOrNull{it.getLong("id")==id}
    }
    fun track():JSONObject? {
        val p=state.project?:return null
        return if(selected==0L)p.getJSONObject("camera").optJSONObject(property)
        else layer(selected)?.getJSONObject("transform")?.optJSONObject(property)
    }
    fun sampleValue():Any? {
        return sampleValueFor(selected,property)
    }
    fun sampleValueFor(objectId:Long,key:String):Any? {
        val s=state.sample?:return null
        return if(objectId==0L)s.optJSONObject("sampledCamera")?.opt(key)
        else s.optJSONArray("sampledLayers")?.let{a->
            (0 until a.length()).map{a.getJSONObject(it)}.firstOrNull{it.getLong("id")==objectId}?.opt(key)}
    }
    fun setValue(value:Any,save:Boolean=true) {
        setPropertyValue(selected,property,floor(frame).toInt(),value,save)
    }
    fun setPropertyValue(objectId:Long,key:String,at:Int,value:Any,save:Boolean=true) {
        edit(JSONObject().put("op",if(value is JSONArray)"set_vector" else "set_scalar")
            .put("object",objectId).put("property",key).put("frame",at).put("value",value),save)
    }
    fun animate() {
        val keys=track()?.optJSONArray("keys")?:return
        edit(JSONObject().put("op","animate").put("object",selected).put("property",property)
            .put("frame",floor(frame).toInt()).put("enabled",keys.length()==0))
    }
    fun addKey() {
        val value=sampleValue()?:return
        val t=track()?:return
        val commands=JSONArray()
        if(t.getJSONArray("keys").length()==0)commands.put(JSONObject().put("op","animate")
            .put("object",selected).put("property",property).put("frame",floor(frame).toInt()).put("enabled",true))
        commands.put(JSONObject().put("op",if(value is JSONArray)"set_vector" else "set_scalar")
            .put("object",selected).put("property",property).put("frame",floor(frame).toInt()).put("value",value))
        editBatch(commands)
    }
    fun keys():List<JSONObject> = track()?.getJSONArray("keys")?.let{a->(0 until a.length()).map{a.getJSONObject(it)}}?:emptyList()
    fun currentKey():JSONObject?=keys().firstOrNull{it.getInt("frame")==floor(frame).toInt()}
    fun toggleKey(){currentKey()?.let{deleteKey(it.getInt("frame"))}?:addKey()}
    fun jumpKey(next:Boolean) {
        val frames=keys().map{it.getInt("frame")}
        val target=if(next)frames.firstOrNull{it>frame}else frames.lastOrNull{it<frame}
        target?.let{seek(it.toDouble())}
    }
    fun deleteKey(key:Int)=edit(JSONObject().put("op","delete_key").put("object",selected).put("property",property).put("frame",key))
    fun moveKey(from:Int,to:Int)=edit(JSONObject().put("op","move_key").put("object",selected).put("property",property).put("from",from).put("to",to))
    fun moveKeyFor(objectId:Long,key:String,from:Int,to:Int)=edit(JSONObject().put("op","move_key").put("object",objectId).put("property",key).put("from",from).put("to",to))
    fun copyKey(from:Int,to:Int)=edit(JSONObject().put("op","copy_key").put("object",selected).put("property",property).put("from",from).put("to",to))
    fun easingSegment():Pair<JSONObject,JSONObject>?=keys().zipWithNext().firstOrNull{(a,b)->frame>=a.getInt("frame")&&frame<b.getInt("frame")}
    fun ease(mode:String) {
        val key=easingSegment()?.first?:return
        edit(JSONObject().put("op","ease").put("object",selected).put("property",property).put("frame",key.getInt("frame")).put("ease",mode))
    }
    fun easingDefinition():JSONObject?=easingSegment()?.first?.let{key->
        JSONObject().put("ease",key.optString("ease","linear")).apply{key.optJSONObject("curve")?.let{put("curve",JSONObject(it.toString()))}}
    }
    fun setCurve(easing:JSONObject,save:Boolean=true) {
        val key=easingSegment()?.first?:return
        if(!editable())return
        edit(JSONObject().put("op","curve").put("object",selected).put("property",property)
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
    fun deleteLayer() {if(selected!=0L||hasCamera()){edit(JSONObject().put("op","remove").put("object",selected).put("frame",floor(frame).toInt()));selected=0L;property="position";panelOpen=false}}
    fun reorder(delta:Int) {
        val p=state.project?:return;val a=p.getJSONArray("layers")
        val index=(0 until a.length()).firstOrNull{a.getJSONObject(it).getLong("id")==selected}?:return
        edit(JSONObject().put("op","reorder").put("object",selected).put("index",(index+delta).coerceIn(0,a.length()-1)))
    }
    fun reorderTo(objectId:Long,index:Int)=edit(JSONObject().put("op","reorder").put("object",objectId).put("index",index))
    private fun nextId(a:JSONArray):Long=(0 until a.length()).maxOfOrNull{a.getJSONObject(it).getLong("id")}?.plus(1)?:1
    private fun channel(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun newLayer(name:String,content:JSONObject,width:Float,height:Float):JSONObject {
        val p=state.project!!
        return JSONObject().put("id",nextId(p.getJSONArray("layers"))).put("name",name).put("content",content)
            .put("size",JSONArray(listOf(width,height))).put("visible",true).put("locked",false)
            .put("transform",JSONObject().put("position",channel(JSONArray(listOf(p.getInt("width")/2f,p.getInt("height")/2f,0))))
                .put("rotation",channel(JSONArray(listOf(0,0,0)))).put("scale",channel(JSONArray(listOf(100,100,100))))
                .put("opacity",channel(1)).put("anchor",JSONArray(listOf(0.5,0.5))))
    }
    fun addRectangle() {
        val p=state.project?:return
        val l=newLayer("矩形",JSONObject().put("kind","solid").put("color",JSONArray(listOf(0.43,0.68,0.91,1))),
            p.getInt("width")*.5f,p.getInt("height")*.2f)
        edit(JSONObject().put("op","add").put("layer",l));selected=l.getLong("id");panelOpen=true
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
                val assetId=nextId(p.getJSONArray("assets"))
                val a=JSONObject().put("id",assetId).put("path","assets/"+file.name).put("width",bitmap.width).put("height",bitmap.height)
                val l=newLayer("图片",JSONObject().put("kind","image").put("asset",assetId),bitmap.width.toFloat(),bitmap.height.toFloat())
                bitmap.recycle()
                withContext(Dispatchers.Main) {
                    editBatch(JSONArray().put(JSONObject().put("op","register_asset").put("asset",a)).put(JSONObject().put("op","add").put("layer",l)))
                    selected=l.getLong("id");panelOpen=true
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
                val aid=nextId(p.getJSONArray("assets"))
                val a=JSONObject().put("id",aid).put("path","assets/"+file.name).put("width",w).put("height",160)
                val content=JSONObject().put("kind","text").put("text",text).put("font","sans-bold")
                    .put("color",JSONArray(listOf(1,1,1,1))).put("raster_asset",aid)
                val l=newLayer("文字",content,w.toFloat(),160f);bitmap.recycle()
                withContext(Dispatchers.Main) {
                    editBatch(JSONArray().put(JSONObject().put("op","register_asset").put("asset",a)).put(JSONObject().put("op","add").put("layer",l)))
                    selected=l.getLong("id");panelOpen=true
                }
            } catch(e:Throwable){fail(e.message?:"文字添加失败")}
        }
    }
    fun output(png:Boolean,callback:(File)->Unit) {
        pause();state=state.copy(busy=true)
        worker.post {
            try {
                val r=JSONObject(if(png)NativeBridge.capture(id)else NativeBridge.pack(id))
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
    fun newProject(width:Int,height:Int,fps:Int=30) {
        pause();state=state.copy(busy=true,error=null)
        val target=JSONArray(listOf(width/2f,height/2f,0))
        val distance=height/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("created",false).put("mode","position").put("position",channel(JSONArray(listOf(width/2f,height/2f,-distance))))
            .put("target",channel(target)).put("roll",channel(0)).put("fov",channel(45))
            .put("radius",channel(distance)).put("azimuth",channel(0)).put("elevation",channel(0))
        val project=JSONObject().put("version",1).put("name","新建工程").put("width",width).put("height",height)
            .put("fps",fps).put("frames",fps*6).put("background",JSONArray(listOf(.05,.06,.09,1)))
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
                    if(JSONObject(result).optBoolean("ok"))main.post{loadFailed=false;selected=0L;property="position";panelOpen=false}
                    publish(result,true)
                }
            }catch(e:Throwable){fail(e.message?:"新建工程失败")}
        }
    }
    fun importProject(uri:Uri) {
        pause();state=state.copy(busy=true)
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
    fun cancelExport() {exporter?.cancelled?.set(true)}
    override fun onCleared() {
        closed.set(true);playing=false
        exporter?.cancelled?.set(true)
        Choreographer.getInstance().removeFrameCallback(tick)
        thermalMonitor.removeThermalStatusListener(thermalListener)
        worker.post{if(id!=0L)NativeBridge.destroy(id);workerThread.quitSafely()}
    }
}
