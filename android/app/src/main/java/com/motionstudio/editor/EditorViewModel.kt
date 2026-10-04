package com.motionstudio.editor

import android.app.Application
import android.graphics.*
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
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
import kotlin.math.*

data class StudioState(
    val project: JSONObject? = null, val sample: JSONObject? = null,
    val canUndo: Boolean = false, val canRedo: Boolean = false,
    val observing: Boolean = false, val error: String? = null,
    val busy: Boolean = false, val saved: Boolean = false,
)

private fun activeProjectDirectory(app:Application):File {
    val saved=app.getSharedPreferences("motion-studio",0).getString("activeProject","default")?:"default"
    val name=if(saved=="default"||saved.matches(Regex("import-[0-9]+")))saved else "default"
    return File(app.filesDir,"studio/"+name)
}

class EditorViewModel @JvmOverloads constructor(app: Application,projectDirectory:File?=null) : AndroidViewModel(app) {
    var state by mutableStateOf(StudioState()); private set
    var frame by mutableDoubleStateOf(0.0); private set
    var playing by mutableStateOf(false); private set
    var selected by mutableLongStateOf(0L)
    var property by mutableStateOf("position")
    var panelOpen by mutableStateOf(false)
    var timelineScale by mutableFloatStateOf(1.5f)
    var exporting by mutableStateOf(false); private set
    var exportProgress by mutableFloatStateOf(0f); private set
    private var exporter:VideoExporter?=null
    private val persistProjectSelection=projectDirectory==null
    var root = (projectDirectory?:activeProjectDirectory(app)).apply { mkdirs() }; private set
    private val workerThread = HandlerThread("motion-render").apply { start() }
    private val worker = Handler(workerThread.looper)
    private val main = Handler(app.mainLooper)
    private var id = 0L
    private val closed = AtomicBoolean(false)
    private val queued = AtomicBoolean(false)
    private val surfaceReady = AtomicBoolean(false)
    private val dirty = AtomicBoolean(true)
    private var startNanos = 0L
    private var startFrame = 0.0
    private val tick = object : Choreographer.FrameCallback {
        override fun doFrame(time: Long) {
            if (closed.get()) return
            val p = state.project
            if (playing && p != null) {
                frame = (startFrame + (time-startNanos)/1e9*p.getInt("fps")) % p.getInt("frames")
                dirty.set(true)
            }
            if (dirty.get() && surfaceReady.get() && queued.compareAndSet(false,true)) {
                dirty.set(false)
                val target=frame
                worker.post {
                    try { if(id!=0L && surfaceReady.get() && !NativeBridge.render(id,target)) dirty.set(true) }
                    catch(e:Throwable) { fail(e.message?:"预览失败") }
                    finally { queued.set(false) }
                }
            }
            Choreographer.getInstance().postFrameCallback(this)
        }
    }
    init {
        worker.post {
            try {
                id=NativeBridge.create(root.absolutePath,"")
                check(id!=0L){"工程无法打开，请先备份工程文件"}
                publish(NativeBridge.state(id),root.resolve("project.json").exists())
            } catch(e:Throwable) {fail(e.message?:"原生引擎初始化失败")}
        }
        Choreographer.getInstance().postFrameCallback(tick)
    }
    private fun fail(message:String) {main.post {state=state.copy(error=message,busy=false)}}
    fun clearError() {state=state.copy(error=null)}
    private fun publish(raw:String,saved:Boolean=false) {
        try {
            val r=JSONObject(raw)
            if(!r.optBoolean("ok")) {fail(r.optString("error","操作失败"));return}
            val d=r.getJSONObject("data")
            main.post {
                if(!closed.get()) {
                    state=StudioState(d.getJSONObject("project"),d,d.optBoolean("canUndo"),
                        d.optBoolean("canRedo"),d.optBoolean("observing"),
                        if(d.isNull("renderError"))null else d.optString("renderError"),false,saved)
                    if(d.has("root")) {
                        root=File(d.getString("root"))
                        if(persistProjectSelection)getApplication<Application>().getSharedPreferences("motion-studio",0)
                            .edit().putString("activeProject",root.name).apply()
                    }
                    if(!playing)frame=d.optDouble("frame",frame)
                    dirty.set(true)
                }
            }
        } catch(e:Throwable){fail(e.message?:"状态读取失败")}
    }
    private fun invoke(save:Boolean=false,operation:()->String) {
        if(closed.get())return
        worker.post {
            if(id==0L||closed.get())return@post
            try {
                val result=operation()
                if(save&&JSONObject(result).optBoolean("ok"))publish(NativeBridge.save(id),true)
                else publish(result)
            } catch(e:Throwable){fail(e.message?:"操作失败")}
        }
    }
    fun attach(surface:Surface,width:Int,height:Int) {
        surfaceReady.set(false)
        invoke {
            val result=NativeBridge.surface(id,surface,width,height)
            surfaceReady.set(JSONObject(result).optBoolean("ok"));result
        }
    }
    fun detach() {
        surfaceReady.set(false)
        worker.post {if(id!=0L)NativeBridge.surface(id,null,0,0)}
    }
    fun pause() {
        if(playing) {playing=false;val f=frame;invoke {NativeBridge.seek(id,f)}}
    }
    fun togglePlay() {
        if(playing)pause() else if(state.project!=null) {
            startFrame=frame;startNanos=System.nanoTime();playing=true
        }
    }
    fun seek(value:Double) {
        pause();val p=state.project?:return
        frame=value.coerceIn(0.0,p.getInt("frames")-1.0)
        val f=frame;invoke {NativeBridge.seek(id,f)}
    }
    fun step(delta:Int)=seek(floor(frame)+delta)
    fun select(objectId:Long) {
        pause();selected=objectId
        property=if(objectId==0L&&state.project?.optJSONObject("camera")?.optString("mode")=="orbit")"radius" else "position"
        panelOpen=true
    }
    fun edit(command:JSONObject,save:Boolean=true) {
        pause();invoke(save){NativeBridge.command(id,command.toString())}
    }
    fun editBatch(commands:JSONArray) {pause();invoke(true){NativeBridge.command(id,commands.toString())}}
    fun undo() {pause();invoke(true){NativeBridge.history(id,0)}}
    fun redo() {pause();invoke(true){NativeBridge.history(id,1)}}
    fun beginGesture() {pause();invoke{NativeBridge.history(id,2)}}
    fun endGesture() {invoke(true){NativeBridge.history(id,3)}}
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
        val s=state.sample?:return null
        return if(selected==0L)s.optJSONObject("sampledCamera")?.opt(property)
        else s.optJSONArray("sampledLayers")?.let{a->
            (0 until a.length()).map{a.getJSONObject(it)}.firstOrNull{it.getLong("id")==selected}?.opt(property)}
    }
    fun setValue(value:Any,save:Boolean=true) {
        edit(JSONObject().put("op",if(value is JSONArray)"set_vector" else "set_scalar")
            .put("object",selected).put("property",property).put("frame",floor(frame).toInt()).put("value",value),save)
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
    fun jumpKey(next:Boolean) {
        val frames=keys().map{it.getInt("frame")}
        val target=if(next)frames.firstOrNull{it>frame}else frames.lastOrNull{it<frame}
        target?.let{seek(it.toDouble())}
    }
    fun deleteKey(key:Int)=edit(JSONObject().put("op","delete_key").put("object",selected).put("property",property).put("frame",key))
    fun moveKey(from:Int,to:Int)=edit(JSONObject().put("op","move_key").put("object",selected).put("property",property).put("from",from).put("to",to))
    fun copyKey(from:Int,to:Int)=edit(JSONObject().put("op","copy_key").put("object",selected).put("property",property).put("from",from).put("to",to))
    fun ease(mode:String) {
        val key=keys().minByOrNull{abs(it.getInt("frame")-frame)}?:return
        edit(JSONObject().put("op","ease").put("object",selected).put("property",property).put("frame",key.getInt("frame")).put("ease",mode))
    }
    fun observe(enabled:Boolean,azimuth:Double=0.0,elevation:Double=0.0) {
        pause();invoke{NativeBridge.observe(id,enabled,azimuth,elevation)}
    }
    fun view(kind:Int) {pause();invoke{NativeBridge.view(id,kind)}}
    fun dolly(amount:Float)=edit(JSONObject().put("op","dolly").put("frame",floor(frame).toInt()).put("amount",amount),false)
    fun pan(x:Float,y:Float)=edit(JSONObject().put("op","pan").put("frame",floor(frame).toInt()).put("x",x).put("y",y),false)
    fun cameraMode(orbit:Boolean)=edit(JSONObject().put("op","camera_mode").put("mode",if(orbit)"orbit" else "position"))
    fun flags(id:Long,visible:Boolean,locked:Boolean)=edit(JSONObject().put("op","flags").put("object",id).put("visible",visible).put("locked",locked))
    fun rename(name:String) {if(selected!=0L)edit(JSONObject().put("op","rename").put("object",selected).put("name",name))}
    fun duplicate() {if(selected!=0L)edit(JSONObject().put("op","duplicate").put("object",selected))}
    fun deleteLayer() {if(selected!=0L){edit(JSONObject().put("op","delete").put("object",selected));selected=0L}}
    fun reorder(delta:Int) {
        val p=state.project?:return;val a=p.getJSONArray("layers")
        val index=(0 until a.length()).firstOrNull{a.getJSONObject(it).getLong("id")==selected}?:return
        edit(JSONObject().put("op","reorder").put("object",selected).put("index",(index+delta).coerceIn(0,a.length()-1)))
    }
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
        pause()
        val target=JSONArray(listOf(width/2f,height/2f,0))
        val distance=height/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("mode","position").put("position",channel(JSONArray(listOf(width/2f,height/2f,-distance))))
            .put("target",channel(target)).put("roll",channel(0)).put("fov",channel(45))
            .put("radius",channel(distance)).put("azimuth",channel(0)).put("elevation",channel(0))
        val project=JSONObject().put("version",1).put("name","新建工程").put("width",width).put("height",height)
            .put("fps",fps).put("frames",fps*6).put("background",JSONArray(listOf(.05,.06,.09,1)))
            .put("camera",camera).put("assets",JSONArray()).put("layers",JSONArray())
        invoke(true){NativeBridge.replace(id,project.toString())}
        selected=0L;property="position";panelOpen=false
    }
    fun importProject(uri:Uri) {
        pause();state=state.copy(busy=true)
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val file=File(root,"incoming-"+UUID.randomUUID()+".motion")
                getApplication<Application>().contentResolver.openInputStream(uri)?.use{input->file.outputStream().use{input.copyTo(it)}}
                    ?:error("无法打开工程文件")
                invoke(true){NativeBridge.importProject(id,file.absolutePath)}
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
        worker.post{if(id!=0L)NativeBridge.destroy(id);workerThread.quitSafely()}
    }
}
