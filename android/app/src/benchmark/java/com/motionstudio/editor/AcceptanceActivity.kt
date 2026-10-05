package com.motionstudio.editor

import android.content.Intent
import java.io.File

/** Same isolated host as debug, compiled into the non-debuggable benchmark. */
class AcceptanceActivity:MainActivity() {
    override fun initialProjectJson():String=if(intent.getBooleanExtra("emptyProject",false))"" else org.json.JSONObject(NativeBridge.projectTemplate(0)).getJSONObject("data").toString()
    companion object {var lastActivityResult:String=""}
    override fun onActivityResult(requestCode:Int,resultCode:Int,data:Intent?) {
        lastActivityResult="request="+requestCode+" result="+resultCode+" flags="+data?.flags+" uri="+data?.data
        super.onActivityResult(requestCode,resultCode,data)
    }
    override fun initialProjectDirectory():File {
        val requested=File(intent.getStringExtra("projectDirectory")?:error("An acceptance project is required")).canonicalFile
        val allowed=File(filesDir,"acceptance").apply{mkdirs()}.canonicalFile
        require(requested.toPath().startsWith(allowed.toPath()))
        return requested
    }
}
