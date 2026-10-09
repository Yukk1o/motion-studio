package com.motionstudio.editor

import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.floor

internal fun EditorViewModel.vectorData(objectId:Long=selected)=if(maskOpen)maskVectorData(objectId)else layer(objectId)?.optJSONObject("content")?.optJSONObject("vector")
internal fun EditorViewModel.vectorSample(objectId:Long=selected)=if(maskOpen)maskVectorSample(objectId)else state.sample?.optJSONArray("vector_layers").objects().firstOrNull{it.getLong("id")==objectId}
internal fun EditorViewModel.shapeCatalogue()=state.sample?.optJSONObject("capabilities")?.optJSONObject("vector_drawing")?.optJSONArray("shape_catalog").objects()
internal fun EditorViewModel.vectorTrackRaw(objectId:Long,key:String):JSONObject? {
    val vector=vectorData(objectId)?:return null
    val parts=key.split(':')
    if(parts.firstOrNull()!="vector")return null
    return when(parts.getOrNull(1)) {
        "parameter"->vector.optJSONObject("source")?.optJSONObject("parameters")?.optJSONObject(parts.getOrNull(2)?:return null)
        "fill"->vector.optJSONObject("fill")
        "stroke_color"->vector.optJSONObject("stroke")?.optJSONObject("color")
        "stroke_width"->vector.optJSONObject("stroke")?.optJSONObject("width")
        "node"->vector.optJSONObject("source")?.optJSONArray("paths").objects().firstOrNull{it.optLong("id")==parts.getOrNull(2)?.toLongOrNull()}
            ?.optJSONArray("nodes").objects().firstOrNull{it.optLong("id")==parts.getOrNull(3)?.toLongOrNull()}?.optJSONObject("geometry")
        else->null
    }
}
internal fun EditorViewModel.vectorValue(objectId:Long,key:String):Any? {
    val sample=vectorSample(objectId)
    val parts=key.split(':')
    return when(parts.getOrNull(1)) {
        "parameter"->sample?.optJSONObject("parameters")?.opt(parts.getOrNull(2)?:return null)
        "fill"->sample?.optJSONArray("fill")
        "stroke_color"->sample?.optJSONObject("stroke")?.optJSONArray("color")
        "stroke_width"->sample?.optJSONObject("stroke")?.opt("width")
        "node"->sample?.optJSONArray("paths").objects().firstOrNull{it.optLong("id")==parts.getOrNull(2)?.toLongOrNull()}
            ?.optJSONArray("nodes").objects().firstOrNull{it.optLong("id")==parts.getOrNull(3)?.toLongOrNull()}?.optJSONArray("geometry")
        else->null
    }?:vectorTrackRaw(objectId,key)?.opt("value")
}
private fun vectorTrack(vector:JSONObject,key:String):JSONObject? {
    val parts=key.split(':')
    return when(parts.getOrNull(1)) {
        "parameter"->vector.optJSONObject("source")?.optJSONObject("parameters")?.optJSONObject(parts.getOrNull(2)?:return null)
        "fill"->vector.optJSONObject("fill")
        "stroke_color"->vector.optJSONObject("stroke")?.optJSONObject("color")
        "stroke_width"->vector.optJSONObject("stroke")?.optJSONObject("width")
        "node"->vector.optJSONObject("source")?.optJSONArray("paths").objects().firstOrNull{it.optLong("id")==parts.getOrNull(2)?.toLongOrNull()}
            ?.optJSONArray("nodes").objects().firstOrNull{it.optLong("id")==parts.getOrNull(3)?.toLongOrNull()}?.optJSONObject("geometry")
        else->null
    }
}

/** Translate timeline operations once; stored vector key times remain layer-local. */
internal fun EditorViewModel.routeVectorCommand(command:JSONObject):JSONObject? {
    val key=command.optString("property")
    if(!key.startsWith("vector:"))return command
    val objectId=command.getLong("object")
    val vector=vectorData(objectId)?.let{JSONObject(it.toString())}?:return null
    val track=vectorTrack(vector,key)?:return null
    val offset=timelineLayer(objectId)?.optInt("offset_frame")?:0
    val at=command.optInt("frame",floor(frame).toInt())-offset
    var keys=track.optJSONArray("keys").objects().map{JSONObject(it.toString())}.toMutableList()
    val sampled=vectorValue(objectId,key)?:track.get("value")
    val discrete=key.startsWith("vector:parameter:")&&shapeCatalogue().firstOrNull{
        it.getString("id")==vector.getJSONObject("source").optString("shape")
    }?.optJSONArray("parameters").objects().firstOrNull{it.getString("id")==key.substringAfterLast(':')}?.optBoolean("discrete")==true
    fun row(value:Any,time:Int)=JSONObject().put("frame",time).put("value",value).put("ease",if(discrete)"hold"else"linear")
    try {
        when(command.getString("op")) {
            "set_scalar","set_vector"->{
                val value=command.get("value")
                if(keys.isEmpty()&&!command.optBoolean("animated"))track.put("value",value)
                else {val existing=keys.firstOrNull{it.getInt("frame")==at};if(existing!=null)existing.put("value",value)else keys.add(row(value,at))}
            }
            "animate"->if(command.getBoolean("enabled")){if(keys.isEmpty())keys.add(row(sampled,at))}else{track.put("value",sampled);keys.clear()}
            "delete_key"->{val removed=keys.removeAll{it.getInt("frame")==at};check(removed){"关键帧已变化，请重试"};if(keys.isEmpty())track.put("value",sampled)}
            "move_key","copy_key"->{
                val from=command.getInt("from")-offset;val to=command.getInt("to")-offset
                val source=keys.firstOrNull{it.getInt("frame")==from}?:error("关键帧已变化，请重试")
                check(to==from||keys.none{it.getInt("frame")==to}){"目标位置已有关键帧"}
                if(command.getString("op")=="move_key")source.put("frame",to)
                else if(to!=from)keys.add(JSONObject(source.toString()).put("frame",to))
            }
            "ease","curve"->{
                check(!discrete){"整数形状参数使用保持插值"}
                val target=keys.firstOrNull{it.getInt("frame")==at}?:error("关键帧已变化，请重试")
                val easing=command.optJSONObject("easing")?:JSONObject().put("ease",command.getString("ease"))
                target.put("ease",easing.getString("ease"));target.remove("curve")
                easing.optJSONObject("curve")?.let{target.put("curve",JSONObject(it.toString()))}
            }
            else->error("此矢量轨道操作暂不可用")
        }
        track.put("keys",JSONArray(keys.sortedBy{it.getInt("frame")}))
        return JSONObject().put("op","vector").put("object",objectId)
            .put("action",JSONObject().put("action","replace").put("vector",vector))
    }catch(error:Throwable){showOperationError(error.message?:"矢量编辑失败");return null}
}

internal fun EditorViewModel.vectorAction(action:JSONObject,save:Boolean=true,objectId:Long=selected) {
    if(maskOpen)maskVectorAction(action,save,objectId)else edit(JSONObject().put("op","vector").put("object",objectId).put("action",action),save)
}
internal fun EditorViewModel.replaceVector(vector:JSONObject,save:Boolean=true)=vectorAction(JSONObject().put("action","replace").put("vector",vector),save)
internal fun EditorViewModel.selectVectorTrack(key:String){if(maskOpen){pause();property="mask:$maskId:node:${key.substringAfterLast(':')}";panelOpen=true;return};if(!vectorOpen)closeWorkspace();pause();property=key;panelOpen=true;vectorOpen=true}
internal fun EditorViewModel.openVector(tab:String="geometry") {
    closeWorkspace();pause();vectorOpen=true;panelOpen=true;vectorTab=tab
    if(!property.startsWith("vector:"))property=if(tab=="style")"vector:fill"else{
        val parameter=vectorData()?.optJSONObject("source")?.optJSONObject("parameters")?.keys()?.asSequence()?.firstOrNull()
        parameter?.let{"vector:parameter:$it"}?:"vector:node:$vectorPathId:$vectorNodeId"
    }
}
internal fun EditorViewModel.addVectorShape(shape:String,name:String) {
    val p=state.project?:return
    val objectId=(p.getJSONArray("layers").objects().maxOfOrNull{it.getLong("id")}?:0L)+1
    edit(JSONObject().put("op","add_shape").put("id",objectId).put("name",name).put("shape",shape)
        .put("size",JSONArray(listOf(240,240))).put("position",JSONArray(listOf(p.getInt("width")/2.0,p.getInt("height")/2.0,0))))
    selected=objectId;property="position";closeWorkspace()
}
internal fun EditorViewModel.addAdjustment() {
    val p=state.project?:return
    val objectId=(p.getJSONArray("layers").objects().maxOfOrNull{it.getLong("id")}?:0L)+1
    edit(JSONObject().put("op","add_adjustment").put("id",objectId).put("name","调整图层"))
    selected=objectId;openEffects()
}
internal fun EditorViewModel.addPenLayer() {
    val p=state.project?:return
    val objectId=(p.getJSONArray("layers").objects().maxOfOrNull{it.getLong("id")}?:0L)+1
    editBatch(JSONArray().put(JSONObject().put("op","add_shape").put("id",objectId).put("name","路径").put("shape","line")
        .put("size",JSONArray(listOf(p.getInt("width"),p.getInt("height")))).put("position",JSONArray(listOf(p.getInt("width")/2.0,p.getInt("height")/2.0,0))))
        .put(JSONObject().put("op","vector").put("object",objectId).put("action",JSONObject().put("action","set_paths")
            .put("paths",JSONArray().put(JSONObject().put("id",1).put("closed",false).put("nodes",JSONArray()))))))
    selected=objectId;vectorPathId=1;vectorNodeId=0;vectorDrawMode=true;openVector()
}

internal fun EditorViewModel.updateVectorPaths(save:Boolean=true,block:(MutableList<JSONObject>)->Unit) {
    val source=vectorData()?.optJSONObject("source")?:return
    if(source.optString("kind")!="paths")return
    val paths=source.optJSONArray("paths").objects().map{JSONObject(it.toString())}.toMutableList()
    block(paths)
    if(paths.size>64||paths.sumOf{it.optJSONArray("nodes")?.length()?:0}>2048){showOperationError("最多支持 64 条路径与 2048 个节点");return}
    vectorAction(JSONObject().put("action","set_paths").put("paths",JSONArray(paths)),save)
}
internal fun EditorViewModel.addVectorPath() {
    val paths=vectorData()?.optJSONObject("source")?.optJSONArray("paths").objects()
    if(paths.size>=64)return
    val pathId=(paths.maxOfOrNull{it.getLong("id")}?:0)+1
    updateVectorPaths{it.add(JSONObject().put("id",pathId).put("closed",false).put("nodes",JSONArray()))}
    vectorPathId=pathId;vectorNodeId=0;vectorDrawMode=true
}
internal fun EditorViewModel.setVectorNode(path:Long,node:Long,geometry:JSONArray,at:Int,save:Boolean=false) {
    vectorAction(JSONObject().put("action","set_node").put("path",path).put("node",node).put("frame",at).put("value",geometry)
        .put("animated",vectorTrackRaw(selected,"vector:node:$path:$node")?.optJSONArray("keys")?.length()?.let{it>0}?:false),save)
}
