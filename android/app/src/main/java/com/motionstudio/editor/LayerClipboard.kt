package com.motionstudio.editor

import org.json.JSONArray
import org.json.JSONObject
import java.io.File

internal data class LayerPaste(val commands:JSONArray,val objects:List<Long>)

/** Immutable snapshots keep copying separate from edits and history. Assets and
 * external parents remain references to the original project. */
internal class LayerClipboard private constructor(
    val root:File,
    private val layers:List<String>,
    private val expressions:List<String>,
    private val assets:List<Pair<String,String>>,
) {
    val composition:String get()=sourceComposition
    private var sourceComposition="comp-main"
    val size:Int get()=layers.size
    fun available(project:JSONObject):Boolean {
        val ids=layers.map{JSONObject(it).getLong("id")}.toSet()
        val current=project.optJSONArray("layers").objects().map{it.getLong("id")}.toSet()
        return layers.all {raw->
            val parent=JSONObject(raw).optJSONObject("parent")
            parent==null||parent.isNull("object")||parent.optLong("object") in ids||
                (if(parent.optLong("object")==0L)project.optJSONObject("camera")?.optBoolean("created")==true else parent.optLong("object") in current)
        }&&assets.all{(kind,raw)->
            val asset=JSONObject(raw)
            project.optJSONArray(kind).objects().any{it.optLong("id")==asset.getLong("id")&&it.toString()==raw}
        }
    }
    fun plan(project:JSONObject,frame:Int):LayerPaste? {
        if(!available(project))return null
        val current=project.getJSONArray("layers").objects()
        val highest=current.maxOfOrNull{it.getLong("id")}?:0L
        if(highest>Long.MAX_VALUE-size)return null
        val copies=layers.map(::JSONObject)
        val ids=copies.mapIndexed{i,l->l.getLong("id") to highest+i+1}.toMap()
        copies.forEach{layer->
            layer.put("id",ids.getValue(layer.getLong("id"))).put("locked",false)
            layer.optJSONObject("parent")?.let{parent->ids[parent.optLong("object")]?.let{parent.put("object",it)}}
        }
        val commands=JSONArray();val added=mutableSetOf<Long>();val pending=copies.toMutableList()
        // Each native command validates the project, so parents must be added first.
        while(pending.isNotEmpty()) {
            val next=pending.firstOrNull {layer->
                val parent=layer.optJSONObject("parent")
                parent==null||parent.isNull("object")||parent.optLong("object") !in ids.values||parent.optLong("object") in added
            }?:return null
            commands.put(JSONObject().put("op","add").put("layer",next))
            added+=next.getLong("id");pending.remove(next)
        }
        // Restore the copied stacking order after adding dependencies.
        copies.forEachIndexed{i,layer->commands.put(JSONObject().put("op","reorder").put("object",layer.getLong("id")).put("index",current.size+i))}
        expressions.forEach{raw->
            val expression=JSONObject(raw)
            val target=expression.getJSONObject("target")
            target.put("object",ids.getValue(target.getLong("object")))
            commands.put(JSONObject().put("op","set_expression").put("expression",expression).put("frame",frame))
        }
        return LayerPaste(commands,copies.map{it.getLong("id")})
    }
    companion object {
        fun capture(root:File,project:JSONObject,objects:Set<Long>):LayerClipboard? {
            val layers=project.getJSONArray("layers").objects().filter{it.getLong("id") in objects}
            if(objects.isEmpty()||layers.size!=objects.size)return null
            val assets=mutableMapOf<Pair<String,Long>,String>()
            fun reference(kind:String,id:Long):Boolean {
                val asset=project.optJSONArray(kind).objects().firstOrNull{it.getLong("id")==id}?:return false
                assets[kind to id]=asset.toString();return true
            }
            for(layer in layers) {
                val content=layer.getJSONObject("content")
                when(content.getString("kind")) {
                    "image"->if(!reference("assets",content.getLong("asset")))return null
                    "text"->if(!content.isNull("raster_asset")&&!reference("assets",content.getLong("raster_asset")))return null
                    "audio"->if(!reference("audio_assets",content.getJSONObject("audio").getLong("asset")))return null
                    "video"->{
                        val id=content.getJSONObject("video").getLong("asset")
                        if(!reference("video_assets",id))return null
                        val asset=project.getJSONArray("video_assets").objects().first{it.getLong("id")==id}
                        if(!asset.isNull("audio_asset")&&!reference("audio_assets",asset.getLong("audio_asset")))return null
                    }
                }
            }
            return LayerClipboard(root,layers.map{it.toString()},project.optJSONArray("expressions").objects()
                .filter{it.getJSONObject("target").getLong("object") in objects}.map{it.toString()},assets.map{(key,value)->key.first to value}).also{it.sourceComposition=project.optString("composition_id","comp-main")}
        }
    }
}
