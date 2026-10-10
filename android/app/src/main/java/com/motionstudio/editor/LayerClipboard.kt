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
        val current=project.optJSONArray("layers").objects().associateBy{it.getLong("id")}
        return layers.all {raw->
            val layer=JSONObject(raw);val parent=layer.optJSONObject("parent")
            val parentAvailable=parent==null||parent.isNull("object")||parent.optLong("object") in ids||
                (if(parent.optLong("object")==0L)project.optJSONObject("camera")?.optBoolean("created")==true else parent.optLong("object") in current)
            val matte=layer.optJSONObject("track_matte")
            val matteAvailable=matte==null||matte.optLong("source") in ids||
                current[matte.optLong("source")]?.getJSONObject("content")?.getString("kind")?.let{it !in setOf("null","audio","adjustment")}==true
            parentAvailable&&matteAvailable&&layer.optJSONArray("effects").objects().all{effect->
                val input=effect.optJSONObject("image_input")
                input==null||input.optString("kind")!="layer"||input.optLong("layer") in ids||
                    current[input.optLong("layer")]?.getJSONObject("content")?.getString("kind")?.let{it !in setOf("null","audio","adjustment")}==true
            }
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
        val materialBindings=JSONArray()
        val matteBindings=JSONArray()
        copies.forEach{layer->
            layer.put("id",ids.getValue(layer.getLong("id"))).put("locked",false)
            layer.optJSONObject("parent")?.let{parent->ids[parent.optLong("object")]?.let{parent.put("object",it)}}
            layer.optJSONObject("track_matte")?.let{matte->
                ids[matte.getLong("source")]?.let{matte.put("source",it)}
                matteBindings.put(JSONObject().put("op","set_track_matte").put("object",layer.getLong("id")).put("matte",matte))
                layer.remove("track_matte")
            }
            layer.optJSONArray("effects").objects().forEach{effect->
                val input=effect.optJSONObject("image_input")
                if(input?.optString("kind")=="layer") {
                    ids[input.getLong("layer")]?.let{input.put("layer",it)}
                    materialBindings.put(JSONObject().put("op","effect").put("object",layer.getLong("id"))
                        .put("action",JSONObject().put("kind","set_image_input").put("effect",effect.getLong("id")).put("input",input)))
                    // Source-stage graphs may contain cycles. Bind only after all
                    // copied layers exist, within the same atomic native batch.
                    effect.put("image_input",JSONObject().put("kind","empty"))
                }
            }
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
        materialBindings.objects().forEach{commands.put(it)}
        matteBindings.objects().forEach{commands.put(it)}
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
                for(effect in layer.optJSONArray("effects").objects()) {
                    val input=effect.optJSONObject("image_input")
                    if(input?.optString("kind")=="asset"&&!reference("assets",input.getLong("asset")))return null
                }
            }
            return LayerClipboard(root,layers.map{it.toString()},project.optJSONArray("expressions").objects()
                .filter{it.getJSONObject("target").getLong("object") in objects}.map{it.toString()},assets.map{(key,value)->key.first to value}).also{it.sourceComposition=project.optString("composition_id","comp-main")}
        }
    }
}
