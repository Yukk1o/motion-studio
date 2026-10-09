package com.motionstudio.editor

import android.content.Context
import android.net.Uri
import android.util.Xml
import org.json.JSONArray
import org.json.JSONObject
import org.xmlpull.v1.XmlPullParser
import java.io.File

/** Backend discovery/import helpers. Call file import on the existing worker. */
internal object SystemFonts {
    private val directories=listOf("/system/fonts","/product/fonts","/system_ext/fonts","/vendor/fonts").map(::File)
    private val configs=listOf("/system/etc/fonts.xml","/product/etc/fonts_customization.xml","/system/etc/system_fonts.xml","/system/etc/font_fallback.xml")
    fun catalogue():String {
        val entries=LinkedHashMap<String,JSONObject>();val errors=JSONArray()
        for(config in configs) {
            val file=File(config);if(!file.isFile||!file.canRead())continue
            try {file.inputStream().use{input->
                val parser=Xml.newPullParser();parser.setInput(input,null)
                var family="";var font:JSONObject?=null;val filename=StringBuilder()
                var event=parser.eventType
                while(event!=XmlPullParser.END_DOCUMENT) {
                    if(event==XmlPullParser.START_TAG)when(parser.name) {
                        "family"->{family=parser.getAttributeValue(null,"name")?:"Fallback"}
                        "font"->{filename.clear();font=JSONObject().put("family",family).put("weight",parser.getAttributeValue(null,"weight")?.toIntOrNull()?:400)
                            .put("style",parser.getAttributeValue(null,"style")?:"normal").put("face_index",parser.getAttributeValue(null,"index")?.toIntOrNull()?:0).put("axes",JSONArray())}
                        "axis"->font?.getJSONArray("axes")?.put(JSONObject().put("tag",parser.getAttributeValue(null,"tag")).put("value",parser.getAttributeValue(null,"stylevalue")))
                    }
                    if(event==XmlPullParser.TEXT&&font!=null)filename.append(parser.text)
                    if(event==XmlPullParser.END_TAG&&parser.name=="font") {
                        val name=filename.toString().trim();val path=directories.map{File(it,name)}.firstOrNull{candidate->
                            candidate.isFile&&candidate.canRead()&&directories.any{root->candidate.canonicalPath.startsWith(root.canonicalPath+File.separator)}}
                        if(path!=null) {
                            val item=font!!;item.put("path",path.canonicalPath).put("source","system").put("name",path.nameWithoutExtension).put("bytes",path.length())
                            entries["${path.canonicalPath}:${item.getInt("face_index")}:${item.getInt("weight")}:${item.getString("style")}"]=item
                        };font=null
                    }
                    event=parser.next()
                }
            }}catch(error:Exception){errors.put(JSONObject().put("config",config).put("message",error.message))}
        }
        // OEMs can supply font files without listing them in the public config.
        for(root in directories)for(file in root.listFiles().orEmpty().sortedBy{it.name}) {
            if(!file.isFile||!file.canRead()||file.extension.lowercase() !in setOf("ttf","otf","ttc"))continue
            if(entries.values.any{it.getString("path")==file.canonicalPath})continue
            entries[file.canonicalPath]=JSONObject().put("family",file.nameWithoutExtension).put("name",file.nameWithoutExtension).put("path",file.canonicalPath)
                .put("weight",400).put("style","normal").put("face_index",0).put("source","system").put("bytes",file.length()).put("axes",JSONArray())
        }
        return JSONObject().put("protocol",1).put("fonts",JSONArray(entries.values.toList())).put("errors",errors).put("variableAxes",false).toString()
    }
    /** A SAF selection is copied into app-private staging, then owned by Rust. */
    fun importUri(id:Long,context:Context,uri:Uri,faceIndex:Int,license:String):String {
        val staging=File.createTempFile("motion-font-",".tmp",context.cacheDir)
        try {
            context.contentResolver.openInputStream(uri).use{source->check(source!=null){"字体文件无法读取"};staging.outputStream().use{output->
                val buffer=ByteArray(65536);var bytes=0L
                while(true){val n=source.read(buffer);if(n<0)break;bytes+=n;check(bytes<=32L*1024*1024){"字体文件超过 32 MiB"};output.write(buffer,0,n)}
            }}
            return NativeBridge.plugin(id,JSONObject().put("op","font_import").put("path",staging.absolutePath).put("face_index",faceIndex).put("license",license).toString())
        }finally{staging.delete()}
    }
}
