package com.motionstudio.editor

import android.annotation.SuppressLint
import android.graphics.Bitmap
import android.net.Uri
import android.webkit.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import java.io.ByteArrayInputStream

internal const val EDITOR_CSP="default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"

/** A per-window HTTPS origin serves only the exact package's declared UI resources. */
internal fun pluginEditorResource(session:PluginEditorSession,uri:Uri):WebResourceResponse {
    val sameOrigin=uri.scheme=="https"&&uri.authority==Uri.parse(session.origin).authority&&uri.query==null&&uri.fragment==null
    val asset=if(sameOrigin)session.assets[uri.encodedPath?.removePrefix("/")]else null
    val headers=mapOf("Content-Security-Policy" to EDITOR_CSP,"X-Content-Type-Options" to "nosniff","Cache-Control" to "no-store")
    return if(asset!=null)WebResourceResponse(asset.mime,"UTF-8",200,"OK",headers,ByteArrayInputStream(asset.bytes))
        else WebResourceResponse("text/plain","UTF-8",403,"Forbidden",headers,ByteArrayInputStream(ByteArray(0)))
}

@SuppressLint("SetJavaScriptEnabled")
@Composable internal fun PluginEditorView(host:PluginEditorHost,session:PluginEditorSession,modifier:Modifier) {
    var view by remember(session.token){mutableStateOf<WebView?>(null)}
    var connected by remember(session.token){mutableStateOf(false)}
    val latestState by rememberUpdatedState(host.state)
    fun call(web:WebView,name:String,value:JSONObject) {
        // JSON is data even when source code or user strings contain quotes and line breaks.
        web.evaluateJavascript("window.$name && window.$name(JSON.parse(${JSONObject.quote(value.toString())}))",null)
    }
    AndroidView(modifier=modifier,factory={context->
        WebView(context).apply {
            view=this
            settings.apply {
                javaScriptEnabled=true;domStorageEnabled=false;allowFileAccess=false;allowContentAccess=false
                mixedContentMode=WebSettings.MIXED_CONTENT_NEVER_ALLOW;blockNetworkLoads=true
                setSupportMultipleWindows(false);javaScriptCanOpenWindowsAutomatically=false
                mediaPlaybackRequiresUserGesture=true
            }
            setDownloadListener{_,_,_,_,_->}
            webChromeClient=WebChromeClient()
            val entry=session.origin+"/"+session.entry
            webViewClient=object:WebViewClient() {
                override fun shouldInterceptRequest(web:WebView,request:WebResourceRequest)=pluginEditorResource(session,request.url)
                override fun shouldOverrideUrlLoading(web:WebView,request:WebResourceRequest)=request.url.toString()!=entry||!request.isForMainFrame
                override fun onPageStarted(web:WebView,url:String?,icon:Bitmap?){connected=false}
                override fun onPageFinished(web:WebView,url:String?) {
                    if(url!=entry)return
                    fun connect(attempt:Int) {
                        if(host.session?.token!=session.token||view!==web)return
                        web.evaluateJavascript("typeof window.motionStudioConnect === 'function'"){ready->
                            if(ready=="true") {
                                call(web,"motionStudioConnect",JSONObject().put("token",session.token).put("definition",session.definition).put("state",latestState?:session.initialState))
                                connected=true
                            }else if(attempt<100)web.postDelayed({connect(attempt+1)},100)
                        }
                    }
                    connect(0)
                }
                override fun onRenderProcessGone(web:WebView,detail:RenderProcessGoneDetail):Boolean {host.close(session.token);return true}
            }
            if(WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
                WebViewCompat.addWebMessageListener(this,"MotionStudioHost",setOf(session.origin)){web,message,origin,isMainFrame,_->
                    if(isMainFrame&&origin.toString().trimEnd('/')==session.origin&&web.url==entry&&host.session?.token==session.token)
                        message.data?.let{raw->host.message(raw){response->if(view===web&&host.session?.token==session.token)call(web,"motionStudioReply",response)}}
                }
                loadUrl(entry)
            }else {
                // The scoped bridge is required. Do not expose an unscoped native interface.
                loadData("<html><body style='color:#eee;background:#1c2128'>请更新系统 WebView 后使用专用编辑器。</body></html>","text/html","UTF-8")
            }
        }
    })
    LaunchedEffect(host.state,connected){if(connected)host.state?.let{value->view?.let{call(it,"motionStudioUpdate",value)}}}
    DisposableEffect(session.token) {onDispose {
        host.close(session.token)
        view?.apply {
            evaluateJavascript("window.motionStudioDisconnect && window.motionStudioDisconnect()",null)
            if(WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER))WebViewCompat.removeWebMessageListener(this,"MotionStudioHost")
            stopLoading();destroy()
        }
        view=null
    }}
}
