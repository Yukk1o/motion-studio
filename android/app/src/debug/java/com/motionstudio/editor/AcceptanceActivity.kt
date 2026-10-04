package com.motionstudio.editor

import java.io.File

/** Debug-only host exercises the real editor and Activity lifecycle with an
 * isolated project, keeping the user's active project and preferences intact. */
class AcceptanceActivity:MainActivity() {
    override fun initialProjectDirectory():File {
        val requested=File(intent.getStringExtra("projectDirectory")?:error("An acceptance project is required")).canonicalFile
        val allowed=File(filesDir,"acceptance").apply{mkdirs()}.canonicalFile
        require(requested.toPath().startsWith(allowed.toPath())){"Acceptance project must stay in the app's test directory"}
        return requested
    }
}
