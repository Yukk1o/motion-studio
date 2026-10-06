package com.motionstudio.editor

import android.app.ActivityManager
import android.app.Application
import android.os.Build
import android.os.SystemClock
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.ArrayDeque
import java.util.UUID

/** Local, bounded diagnostics. Never records commands, project data, or media bytes. */
internal class ErrorReports(private val app:Application,private val folder:File=File(app.filesDir,"diagnostics")) {
    private val operations=ArrayDeque<JSONObject>()
    private var lastMessage:String?=null
    private var lastAt=0L
    @Synchronized fun operation(name:String) {
        operations.addLast(JSONObject().put("at",System.currentTimeMillis()).put("operation",name.take(96)))
        while(operations.size>40)operations.removeFirst()
    }
    @Synchronized fun record(message:String,cause:Throwable?=null,context:JSONObject=JSONObject()):File? {
        val now=SystemClock.elapsedRealtime()
        if(message==lastMessage&&now-lastAt<5000)return latest()
        lastMessage=message;lastAt=now
        return runCatching {
            folder.mkdirs()
            val report=base().put("id",UUID.randomUUID().toString()).put("created_at",System.currentTimeMillis())
                .put("error",redactDiagnostic(message)).put("context",context)
                .put("recent_operations",JSONArray(operations.toList()))
            cause?.let{report.put("exception_type",it.javaClass.name).put("stack",JSONArray(it.stackTrace.take(48).map{frame->frame.toString()}))}
            val file=File(folder,"error-${System.currentTimeMillis()}-${UUID.randomUUID()}.json")
            val pending=File(folder,file.name+".tmp")
            pending.writeText(report.toString(2));check(pending.renameTo(file))
            folder.listFiles().orEmpty().filter{it.extension=="json"}.sortedByDescending{it.lastModified()}.drop(8).forEach{it.delete()}
            file
        }.getOrNull()
    }
    @Synchronized fun latest():File?=folder.listFiles().orEmpty().filter{it.extension=="json"}.maxByOrNull{it.lastModified()}
    fun export():File {
        val current=latest()
        if(current!=null)return current
        return record("用户生成诊断报告")?:error("错误报告暂时无法保存")
    }
    private fun base():JSONObject {
        val info=app.packageManager.getPackageInfo(app.packageName,0)
        val memory=ActivityManager.MemoryInfo().also{app.getSystemService(ActivityManager::class.java).getMemoryInfo(it)}
        val display=app.resources.displayMetrics
        val exits=if(Build.VERSION.SDK_INT>=30)runCatching{app.getSystemService(ActivityManager::class.java)
            .getHistoricalProcessExitReasons(app.packageName,0,3).map{JSONObject().put("reason",it.reason).put("status",it.status).put("at",it.timestamp)}}.getOrDefault(emptyList())else emptyList()
        return JSONObject().put("schema_version",1).put("app",JSONObject().put("package",app.packageName)
            .put("version",info.versionName).put("version_code",info.longVersionCode))
            .put("device",JSONObject().put("manufacturer",Build.MANUFACTURER).put("model",Build.MODEL)
                .put("android",Build.VERSION.RELEASE).put("api",Build.VERSION.SDK_INT).put("abis",JSONArray(Build.SUPPORTED_ABIS.toList()))
                .put("display",JSONArray(listOf(display.widthPixels,display.heightPixels))).put("density",display.densityDpi)
                .put("font_scale",app.resources.configuration.fontScale).put("low_memory",memory.lowMemory))
            .put("previous_process_exits",JSONArray(exits)).put("includes_project_or_media",false)
    }
}

internal fun redactDiagnostic(text:String):String = text.take(8192)
    .replace(Regex("(?i)(?:content|file|https?)://[^\\s\\\"<>]+"),"[地址已隐藏]")
    .replace(Regex("(?:[A-Za-z]:[\\\\/]|/(?:data|storage|sdcard|mnt|home)/)[^\\s\\\"<>]+"),"[路径已隐藏]")
    .replace(Regex("(?i)(token|password|authorization|api[_-]?key)\\s*[:=]\\s*[^\\s,;]+"),"$1=[已隐藏]")

class StudioApplication:Application() {
    internal lateinit var errors:ErrorReports;private set
    override fun onCreate() {
        super.onCreate();errors=ErrorReports(this)
        val previous=Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler{thread,error->
            errors.record("未捕获异常："+(error.message?:error.javaClass.simpleName),error)
            previous?.uncaughtException(thread,error)
        }
    }
}
