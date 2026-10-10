package com.motionstudio.editor

import org.json.JSONObject

internal fun findGroupItem(group:JSONObject,id:Long):JSONObject? {
    if(group.optLong("id")==id)return group
    for(item in group.optJSONArray("items").objects()) {
        if(item.optString("kind")=="group")findGroupItem(item.getJSONObject("group"),id)?.let{return it}
        else if(item.optLong("id")==id)return item
    }
    return null
}
internal fun groupTrack(vector:JSONObject,id:Long,parameter:String):JSONObject? {
    val group=vector.optJSONObject("source")?.optJSONObject("group")?:return null
    val item=findGroupItem(group,id)?:return null
    if(item.has("items"))return item.optJSONObject("transform")?.optJSONObject(parameter)
    return when(item.optString("kind")) {
        "geometry"->{val v=item.getJSONObject("vector")
            when(parameter){"size","position"->item.optJSONObject(parameter)
                "fill"->v.optJSONObject("fill")
                "stroke_color"->v.optJSONObject("stroke")?.optJSONObject("color")
                "stroke_width"->v.optJSONObject("stroke")?.optJSONObject("width")
                "trim_start","trim_end","trim_offset"->v.optJSONObject("trim")?.optJSONObject(parameter.removePrefix("trim_"))
                "dash_offset"->v.optJSONObject("stroke")?.optJSONObject("dashes")?.optJSONObject("offset")
                else->when {
                    parameter.startsWith("node:")->{
                        val parts=parameter.split(':')
                        if(parts.size!=3)null else v.optJSONObject("source")?.optJSONArray("paths").objects()
                            .firstOrNull{it.optLong("id")==parts[1].toLongOrNull()}?.optJSONArray("nodes").objects()
                            .firstOrNull{it.optLong("id")==parts[2].toLongOrNull()}?.optJSONObject("geometry")
                    }
                    parameter.startsWith("shape:")->v.optJSONObject("source")?.optJSONObject("parameters")?.optJSONObject(parameter.removePrefix("shape:"))
                    else->parameter.takeIf{it.startsWith("dash_")}?.removePrefix("dash_")?.toIntOrNull()?.let{v.optJSONObject("stroke")?.optJSONObject("dashes")?.optJSONArray("pattern")?.optJSONObject(it)}
                }
            }}
        "fill"->item.optJSONObject(parameter)
        "stroke"->{val stroke=item.optJSONObject("stroke")
            when {
                parameter=="dash_offset"->stroke?.optJSONObject("dashes")?.optJSONObject("offset")
                parameter.startsWith("dash_")->parameter.removePrefix("dash_").toIntOrNull()?.let{stroke?.optJSONObject("dashes")?.optJSONArray("pattern")?.optJSONObject(it)}
                else->stroke?.optJSONObject(parameter)
            }}
        "trim"->item.optJSONObject("trim")?.optJSONObject(parameter)
        "repeater"->item.optJSONObject("repeater")?.optJSONObject(parameter)
        else->null
    }
}
internal fun groupTrackDiscrete(vector:JSONObject,key:String,catalog:List<JSONObject>):Boolean {
    val parts=key.split(':');if(parts.getOrNull(1)!="group"||parts.getOrNull(3)!="shape")return false
    val root=vector.optJSONObject("source")?.optJSONObject("group")?:return false
    val item=findGroupItem(root,parts.getOrNull(2)?.toLongOrNull()?:return false)?:return false
    val shape=item.optJSONObject("vector")?.optJSONObject("source")?.optString("shape")?:return false
    return catalog.firstOrNull{it.optString("id")==shape}?.optJSONArray("parameters").objects()
        .firstOrNull{it.optString("id")==parts.getOrNull(4)}?.optBoolean("discrete")==true
}
internal fun nextGroupId(root:JSONObject):Long {
    fun maximum(group:JSONObject):Long=maxOf(group.optLong("id"),group.optJSONArray("items").objects().maxOfOrNull{
        if(it.optString("kind")=="group")maximum(it.getJSONObject("group"))else it.optLong("id")
    }?:0)
    return Math.addExact(maximum(root),1L)
}
