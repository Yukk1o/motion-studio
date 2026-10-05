package com.motionstudio.editor

import java.io.File
import android.content.Intent

/** Debug-only host exercises the real editor and Activity lifecycle with an
 * isolated project, keeping the user's active project and preferences intact. */
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
        require(requested.toPath().startsWith(allowed.toPath())){"Acceptance project must stay in the app's test directory"}
        return requested
    }
}
