package com.motionstudio.editor

import android.os.Handler
import android.os.SystemClock
import android.util.Base64
import androidx.compose.runtime.*
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.atomic.AtomicLong

internal data class PluginEditorAsset(val mime:String,val bytes:ByteArray)
internal data class PluginEditorSession(val token:String,val origin:String,val entry:String,val definition:JSONObject,
    val initialState:JSONObject,val assets:Map<String,PluginEditorAsset>)

/** All engine access, including closing a gesture, stays on the render worker. */
internal class PluginEditorHost(private val worker:Handler,private val main:Handler,private val handle:()->Long,
    private val closed:()->Boolean,private val composition:()->String,private val changed:(Boolean)->Unit) {
    var session by mutableStateOf<PluginEditorSession?>(null);private set
    var state by mutableStateOf<JSONObject?>(null);private set
    var error by mutableStateOf<String?>(null);private set
    var loading by mutableStateOf(false);private set
    @Volatile var gesture=false;private set
    private val generation=AtomicLong()
    private var workerToken:String?=null
    private var lastPreview=0L
    private val outstanding=mutableSetOf<String>() // Main thread only.
    private fun data(raw:String):JSONObject {
        val envelope=JSONObject(raw);check(envelope.optBoolean("ok")){envelope.optString("error","编辑器操作失败")}
        return envelope.getJSONObject("data")
    }
    private fun native(request:JSONObject)=CompositionBridge.plugin(handle(),composition(),request)
    private fun closeOnWorker(commit:Boolean=false) {
        workerToken?.let{token->runCatching{native(JSONObject().put("op","editor_close").put("token",token).put("commit",commit))}}
        workerToken=null
    }
    fun open(objectId:Long,instance:Long) {
        close();val ticket=generation.get();loading=true;error=null
        worker.post {
            if(ticket!=generation.get()||closed()||handle()==0L)return@post
            try {
                val connection=data(native(JSONObject().put("op","editor_open").put("object",objectId).put("instance",instance)))
                val token=connection.getString("token");workerToken=token
                check(connection.getInt("protocol")==1){"编辑器协议暂不支持"}
                val definition=connection.getJSONObject("definition")
                if(definition.optJSONObject("native_editor")!=null) {
                    check(definition.getJSONObject("native_editor").getInt("protocol")==1){"原生编辑器协议暂不支持"}
                    val initial=data(native(JSONObject().put("op","editor_message").put("token",token).put("message",JSONObject().put("op","begin").put("revision",connection.getJSONObject("state").getLong("revision")))))
                    val result=PluginEditorSession(token,"","",definition,initial,emptyMap())
                    main.post{if(ticket==generation.get()&&!closed()){session=result;state=initial;gesture=true;loading=false}}
                    return@post
                }
                val editor=definition.getJSONObject("editor")
                val files=editor.getJSONArray("files");check(files.length() in 1..32){"编辑器资源数量无效"}
                val assets=linkedMapOf<String,PluginEditorAsset>();var total=0
                for(i in 0 until files.length()) {
                    val path=files.getString(i)
                    check(path.startsWith("ui/")&&!path.contains("..")&&!path.contains('\\')&&!path.contains('%')){"编辑器资源路径无效"}
                    val asset=data(native(JSONObject().put("op","editor_asset").put("token",token).put("path",path)))
                    val bytes=Base64.decode(asset.getString("base64"),Base64.DEFAULT);total+=bytes.size
                    check(total<=4*1024*1024){"编辑器资源超过 4 MiB"}
                    assets[path]=PluginEditorAsset(asset.getString("mime"),bytes)
                }
                val entry=editor.getString("entry");check(assets.containsKey(entry)){"编辑器缺少入口文件"}
                val result=PluginEditorSession(token,"https://editor-${UUID.randomUUID()}.motionstudio.invalid",entry,definition,connection.getJSONObject("state"),assets)
                main.post{if(ticket==generation.get()&&!closed()){session=result;state=result.initialState;loading=false}}
            }catch(e:Throwable){closeOnWorker();main.post{if(ticket==generation.get()){gesture=false;error=e.message?:"编辑器无法打开";loading=false}}}
        }
    }
    fun close(token:String?=null,commit:Boolean=false) {
        if(token!=null&&session?.token!=token)return
        generation.incrementAndGet();session=null;state=null;loading=false;gesture=false;outstanding.clear()
        worker.post{closeOnWorker(commit);if(!closed()&&handle()!=0L)changed(commit)}
    }
    fun refresh() {
        val ticket=generation.get()
        worker.post {
            val token=workerToken?:return@post
            if(ticket!=generation.get()||closed())return@post
            runCatching{data(native(JSONObject().put("op","editor_message").put("token",token).put("message",JSONObject().put("op","state"))))}
                .onSuccess{next->main.post{if(ticket==generation.get())state=next}}
                .onFailure{failure->closeOnWorker();main.post{if(ticket==generation.get()){gesture=false;session=null;state=null;error=failure.message}}}
        }
    }
    fun message(raw:String,reply:(JSONObject)->Unit) {
        val current=session?:return
        if(raw.toByteArray(Charsets.UTF_8).size>256*1024)return
        val envelope=runCatching{JSONObject(raw)}.getOrNull()?:return
        val requestId=envelope.optString("id")
        if(envelope.optInt("protocol")!=1||envelope.optString("token")!=current.token||requestId.length !in 1..128)return
        val request=envelope.optJSONObject("message")?:return
        val op=request.optString("op")
        fun reject(message:String){reply(JSONObject().put("token",current.token).put("id",requestId).put("ok",false).put("error",message))}
        if(op !in setOf("state","preview","set","animate","key","curve","scene","seed","transform","begin","commit","cancel","color_begin","color_finish")){reject("编辑器操作不支持");return}
        if(requestId in outstanding||outstanding.size>=8){reject("请等待当前编辑完成");return}
        if(op=="preview") {
            if(request.optInt("width") !in 1..512||request.optInt("height") !in 1..512){reject("预览尺寸必须是 1～512");return}
            val now=SystemClock.elapsedRealtime()
            if(now-lastPreview<100){reject("预览请求过于频繁");return}
            lastPreview=now
        }
        if(op=="begin")gesture=true
        outstanding.add(requestId);val ticket=generation.get()
        worker.post {
            if(ticket!=generation.get()||workerToken!=current.token||closed())return@post
            val result=runCatching{JSONObject(native(JSONObject().put("op","editor_message").put("token",current.token).put("message",request)))}
                .getOrElse{JSONObject().put("ok",false).put("error",it.message?:"编辑器操作失败")}
            val ok=result.optBoolean("ok")
            val next=result.optJSONObject("data")
            if(op!="preview")gesture=if(ok)next?.optBoolean("gesture",false)==true else op!="begin"&&gesture
            if(ok&&op !in setOf("preview","state","begin"))changed(!gesture&&op!="cancel")
            val response=JSONObject().put("token",current.token).put("id",requestId).put("ok",ok)
            if(ok)response.put("result",next)else response.put("error",result.optString("error","编辑器操作失败"))
            main.post {
                if(ticket==generation.get()&&session?.token==current.token&&!closed()) {
                    outstanding.remove(requestId)
                    if(ok&&op!="preview")state=next
                    reply(response)
                }
            }
        }
    }
    fun request(request:JSONObject,reply:(JSONObject)->Unit) {
        val current=session?:return
        message(JSONObject().put("protocol",1).put("token",current.token).put("id",UUID.randomUUID().toString()).put("message",request).toString(),reply)
    }
}
