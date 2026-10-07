package com.motionstudio.editor

import android.app.Application
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

internal const val RELEASE_PAGE="https://github.com/Yukk1o/motion-studio/releases"
internal data class ReleaseNotice(val version:String,val code:Long,val notes:String,val url:String,val preview:Boolean) {
    val key:String get()="$version:$code"
    fun json()=JSONObject().put("version",version).put("code",code).put("notes",notes).put("url",url).put("preview",preview)
}
internal fun parseRelease(release:JSONObject,info:JSONObject,preview:Boolean):ReleaseNotice? {
    if(release.optBoolean("draft")||release.optBoolean("prerelease")!=preview)return null
    if(preview&&release.optString("tag_name")!="preview")return null
    val url=release.optString("html_url")
    if(!url.startsWith("$RELEASE_PAGE/tag/"))return null
    val code=info.optLong("versionCode",0);val version=info.optString("versionName")
    if(code !in 1..2100000000L||version.isBlank()||info.optString("kind")!=if(preview)"preview"else"release")return null
    if(!release.optJSONArray("assets").objects().any{it.optString("name")==info.optString("apk")&&it.optString("name").endsWith(".apk")})return null
    return ReleaseNotice(version,code,release.optString("body").take(32768),url,preview)
}

/** No request is made on the UI thread; failure leaves the last cached result intact. */
internal class ReleaseUpdates(private val app:Application,private val scope:CoroutineScope,private val fetch:(suspend (Boolean)->ReleaseNotice?)?=null,automatic:Boolean=true) {
    var release by mutableStateOf<ReleaseNotice?>(null);private set
    var checking by mutableStateOf(false);private set
    var status by mutableStateOf<String?>(null);private set
    private val prefs=app.getSharedPreferences("motion-release-updates",0)
    private var dismissed by mutableStateOf(prefs.getString("dismissed",null))
    private val installed=app.packageManager.getPackageInfo(app.packageName,0)
    private val preview=installed.versionName.orEmpty().contains("-preview.")||app.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE!=0
    private val channel=if(preview)"preview"else"release"
    val available:Boolean get()=release?.let{it.code>installed.longVersionCode&&dismissed!=it.key}==true
    val installedVersion:String get()=installed.versionName.orEmpty()
    init {
        release=runCatching{JSONObject(prefs.getString("cached-$channel",null)?:return@runCatching null).let{entry->
            kotlin.check(entry.getString("url").startsWith("$RELEASE_PAGE/tag/")&&entry.getBoolean("preview")==preview&&entry.getLong("code") in 1..2100000000L)
            ReleaseNotice(entry.getString("version"),entry.getLong("code"),entry.getString("notes"),entry.getString("url"),entry.getBoolean("preview"))
        }}.getOrNull()
        if(automatic)check()
    }
    fun dismiss(){release?.let{dismissed=it.key;prefs.edit().putString("dismissed",it.key).apply()}}
    fun check(manual:Boolean=false) {
        if(checking)return
        val elapsed=System.currentTimeMillis()-prefs.getLong("checked-$channel",0)
        if(!manual&&elapsed in 0 until 6*60*60*1000L)return
        checking=true;status=null
        scope.launch {
            try {
                val result=withContext(Dispatchers.IO) {if(fetch!=null)fetch.invoke(preview)else {
                    val api="https://api.github.com/repos/Yukk1o/motion-studio/releases"+(if(preview)"/tags/preview"else"?per_page=10")
                    val raw=get(api,"api-$channel",512*1024)
                    val candidates=if(preview)listOf(JSONObject(raw))else JSONArray(raw).objects().filter{!it.optBoolean("draft")&&!it.optBoolean("prerelease")}
                    candidates.firstOrNull{entry->entry.optJSONArray("assets").objects().any{it.optString("name")=="build-info.json"}}?.let{entry->
                        val asset=entry.optJSONArray("assets").objects().firstOrNull{it.optString("name")=="build-info.json"}?:return@let null
                        val address=asset.optString("browser_download_url")
                        kotlin.check(address.startsWith("https://github.com/Yukk1o/motion-studio/releases/download/"))
                        parseRelease(entry,JSONObject(get(address,"info-$channel",64*1024)),preview)
                    }
                }}
                release=result
                val edit=prefs.edit().putLong("checked-$channel",System.currentTimeMillis())
                if(result==null)edit.remove("cached-$channel")else edit.putString("cached-$channel",result.json().toString())
                edit.apply()
                if(manual)status=if(result==null)"暂无可用版本"else if(result.code>installed.longVersionCode)"有新版本可查看"else"当前已是最新版本"
            }catch(cancelled:CancellationException){throw cancelled}
            catch(_:Exception){if(manual)status="暂时无法检查更新，稍后重试"}
            finally {checking=false}
        }
    }
    private fun get(address:String,key:String,maximum:Int):String {
        val connection=URL(address).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout=6000;connection.readTimeout=6000
            connection.setRequestProperty("Accept",if(address.contains("api.github.com"))"application/vnd.github+json"else"application/json")
            connection.setRequestProperty("User-Agent","MotionStudio/${installed.versionName}")
            connection.setRequestProperty("X-GitHub-Api-Version","2026-03-10")
            prefs.getString("etag-$key",null)?.let{connection.setRequestProperty("If-None-Match",it)}
            if(connection.responseCode==304)return prefs.getString("body-$key",null)?:error("缓存不存在")
            kotlin.check(connection.responseCode==200)
            kotlin.check(connection.contentLengthLong<=maximum)
            val bytes=connection.inputStream.use{input->
                val output=java.io.ByteArrayOutputStream();val buffer=ByteArray(4096)
                while(output.size()<=maximum){val count=input.read(buffer,0,minOf(buffer.size,maximum+1-output.size()));if(count<0)break;output.write(buffer,0,count)}
                output.toByteArray()
            };kotlin.check(bytes.size<=maximum)
            val body=bytes.toString(Charsets.UTF_8)
            prefs.edit().putString("body-$key",body).putString("etag-$key",connection.getHeaderField("ETag")).apply()
            return body
        }finally{connection.disconnect()}
    }
}

@Composable internal fun ReleaseUpdateBanner(updates:ReleaseUpdates,onOpen:()->Unit) {
    if(!updates.available)return
    Row(Modifier.fillMaxWidth().background(Accent.copy(alpha=.08f)).padding(horizontal=16.dp).heightIn(min=48.dp).testTag("update-banner"),verticalAlignment=Alignment.CenterVertically) {
        Text("新版本 · ${updates.release?.version}",Modifier.weight(1f),fontSize=13.sp,maxLines=2)
        TextButton(onClick=onOpen,modifier=Modifier.heightIn(min=48.dp)){Text("查看更新")}
        TextButton(onClick=updates::dismiss,modifier=Modifier.heightIn(min=48.dp).testTag("dismiss-update")){Text("稍后")}
    }
}

@Composable internal fun ReleaseNotes(updates:ReleaseUpdates,onDismiss:()->Unit) {
    val context=androidx.compose.ui.platform.LocalContext.current
    val release=updates.release
    AlertDialog(onDismissRequest=onDismiss,title={Text("更新日志")},text={Column(Modifier.heightIn(max=440.dp).verticalScroll(rememberScrollState()).testTag("release-notes")) {
        Text("当前版本 · ${updates.installedVersion}",color=Muted,fontSize=12.sp)
        if(release!=null){Spacer(Modifier.height(16.dp));Text(release.version,color=Accent);Spacer(Modifier.height(8.dp));Text(release.notes.ifBlank{"此版本未提供更新说明。"},fontSize=14.sp)}
        else Text("暂无已缓存的更新日志。",Modifier.padding(vertical=16.dp),color=Muted)
        if(updates.checking)LinearProgressIndicator(Modifier.fillMaxWidth())
        updates.status?.let{Text(it,color=Muted,fontSize=12.sp)}
        TextButton(onClick={updates.check(true)},enabled=!updates.checking,modifier=Modifier.testTag("check-updates")){Text("检查更新")}
    }},confirmButton={TextButton(onClick={runCatching{context.startActivity(Intent(Intent.ACTION_VIEW,Uri.parse(release?.url?:RELEASE_PAGE)))}}){Text(if(release==null)"打开发布页"else"查看发布页")}},dismissButton={TextButton(onClick=onDismiss){Text("关闭")}})
}
