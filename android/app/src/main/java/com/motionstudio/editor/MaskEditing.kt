package com.motionstudio.editor

import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.floor

internal fun EditorViewModel.masks(objectId:Long=selected)=layer(objectId)?.optJSONArray("masks").objects()
internal fun EditorViewModel.maskData(objectId:Long=selected,id:Long=maskId)=masks(objectId).firstOrNull{it.getLong("id")==id}
internal fun EditorViewModel.maskSample(objectId:Long=selected)=state.sample?.optJSONArray("mask_layers").objects().firstOrNull{it.getLong("id")==objectId}
internal fun maskTarget(key:String):Pair<Long,String>? {
    val p=key.split(':');if(p.firstOrNull()!="mask")return null
    return (p.getOrNull(1)?.toLongOrNull()?:return null) to p.drop(2).joinToString(":")
}
internal fun maskTrack(mask:JSONObject,key:String):JSONObject? = if(key.startsWith("node:"))
    mask.getJSONObject("path").getJSONArray("nodes").objects().firstOrNull{it.getLong("id")==key.substringAfter(':').toLongOrNull()}?.optJSONObject("geometry")
    else mask.optJSONObject(key)
internal fun EditorViewModel.maskTrackRaw(objectId:Long,key:String):JSONObject? {
    val (id,property)=maskTarget(key)?:return null
    return maskTrack(maskData(objectId,id)?:return null,property)
}
internal fun EditorViewModel.maskValue(objectId:Long,key:String):Any? {
    val (id,property)=maskTarget(key)?:return null
    val m=maskSample(objectId)?.optJSONArray("masks").objects().firstOrNull{it.getLong("id")==id}
    return if(property.startsWith("node:"))m?.optJSONObject("path")?.optJSONArray("nodes").objects().firstOrNull{it.getLong("id")==property.substringAfter(':').toLongOrNull()}?.optJSONArray("geometry")?:maskTrackRaw(objectId,key)?.opt("value")
        else m?.opt(property)?:maskTrackRaw(objectId,key)?.opt("value")
}
internal fun EditorViewModel.maskAction(action:JSONObject,save:Boolean=true,objectId:Long=selected)=edit(JSONObject().put("op","mask").put("object",objectId).put("action",action),save)
internal fun EditorViewModel.openMasks() {
    closeWorkspace();pause();maskOpen=true;panelOpen=true
    maskId=masks().firstOrNull()?.getLong("id")?:0
    property="mask:$maskId:opacity";vectorPathId=maskData()?.getJSONObject("path")?.getLong("id")?:1;vectorNodeId=0;vectorDrawMode=false
}
internal fun EditorViewModel.chooseMask(id:Long){pause();maskId=id;property="mask:$id:opacity";vectorPathId=maskData()?.getJSONObject("path")?.getLong("id")?:1;vectorNodeId=0;vectorDrawMode=false}
internal fun EditorViewModel.addMask(pen:Boolean=false) {
    val size=layer(selected)?.getJSONArray("size")?:return
    val width=size.getDouble(0);val height=size.getDouble(1)
    val id=(masks().maxOfOrNull{it.getLong("id")}?:0)+1
    val points=if(pen)emptyList()else listOf(width*.2 to height*.2,width*.8 to height*.2,width*.8 to height*.8,width*.2 to height*.8)
    val nodes=JSONArray(points.mapIndexed{i,p->JSONObject().put("id",i+1).put("geometry",JSONObject().put("value",JSONArray(listOf(p.first,p.second,0,0,0,0))).put("keys",JSONArray()))})
    val m=JSONObject().put("id",id).put("name","蒙版 $id").put("path",JSONObject().put("id",1).put("closed",!pen).put("nodes",nodes))
    maskAction(JSONObject().put("kind","add").put("mask",m));maskId=id;property="mask:$id:opacity";vectorPathId=1;vectorNodeId=0;vectorDrawMode=pen
}
internal fun EditorViewModel.routeMaskCommand(command:JSONObject):JSONObject? {
    val (id,key)=maskTarget(command.optString("property"))?:return command
    val objectId=command.getLong("object")
    val property:Any=if(key.startsWith("node:"))JSONObject().put("node",key.substringAfter(':').toLong())else key
    val action=JSONObject().put("mask",id).put("property",property)
    when(command.getString("op")) {
        "set_scalar","set_vector"->{action.put("kind","set").put("frame",command.getInt("frame"));val value=command.get("value");action.put("value",if(value is JSONArray)value else JSONArray().put(value));if(command.has("animated"))action.put("animated",command.getBoolean("animated"))}
        "animate"->action.put("kind","animate").put("frame",command.getInt("frame")).put("enabled",command.getBoolean("enabled"))
        "delete_key"->action.put("kind","delete_key").put("frame",command.getInt("frame"))
        "move_key","copy_key"->action.put("kind",command.getString("op")).put("from",command.getInt("from")).put("to",command.getInt("to"))
        "ease","curve"->action.put("kind","curve").put("frame",command.getInt("frame")).put("easing",command.optJSONObject("easing")?:JSONObject().put("ease",command.getString("ease")))
        else->{showOperationError("此蒙版轨道操作暂不可用");return null}
    }
    return JSONObject().put("op","mask").put("object",objectId).put("action",action)
}

/** Adapt only the existing pen tool's geometry; stored mask pixels stay top-left. */
internal fun EditorViewModel.maskVectorData(objectId:Long):JSONObject? {
    val mask=maskData(objectId)?:return null;val p=JSONObject(mask.getJSONObject("path").toString())
    val size=layer(objectId)?.getJSONArray("size")?:return null
    p.getJSONArray("nodes").objects().forEach{n->val t=n.getJSONObject("geometry");fun shift(a:JSONArray){a.put(0,a.getDouble(0)-size.getDouble(0)/2);a.put(1,a.getDouble(1)-size.getDouble(1)/2)};shift(t.getJSONArray("value"));t.optJSONArray("keys").objects().forEach{shift(it.getJSONArray("value"))}}
    return JSONObject().put("source",JSONObject().put("kind","paths").put("paths",JSONArray().put(p)))
}
internal fun EditorViewModel.maskVectorSample(objectId:Long):JSONObject? {
    val sample=maskSample(objectId)?:return null
    val mask=sample.getJSONArray("masks").objects().firstOrNull{it.getLong("id")==maskId}?:return null
    val p=JSONObject(mask.getJSONObject("path").toString());val size=layer(objectId)?.getJSONArray("size")?:return null
    p.getJSONArray("nodes").objects().forEach{n->val a=n.getJSONArray("geometry");a.put(0,a.getDouble(0)-size.getDouble(0)/2);a.put(1,a.getDouble(1)-size.getDouble(1)/2)}
    return JSONObject().put("id",objectId).put("mvp",sample.getJSONArray("mvp")).put("paths",JSONArray().put(p))
}
internal fun EditorViewModel.maskVectorAction(action:JSONObject,save:Boolean,objectId:Long) {
    val mask=maskData(objectId)?:return;val size=layer(objectId)?.getJSONArray("size")?:return
    when(action.getString("action")) {
        "set_paths"->{val path=action.getJSONArray("paths").optJSONObject(0)?:return;val copy=JSONObject(path.toString());copy.getJSONArray("nodes").objects().forEach{n->val t=n.getJSONObject("geometry");fun shift(a:JSONArray){a.put(0,a.getDouble(0)+size.getDouble(0)/2);a.put(1,a.getDouble(1)+size.getDouble(1)/2)};shift(t.getJSONArray("value"));t.optJSONArray("keys").objects().forEach{shift(it.getJSONArray("value"))}};maskAction(JSONObject().put("kind","path").put("mask",maskId).put("path",copy),save,objectId)}
        "set_node"->{val g=JSONArray(action.getJSONArray("value").toString()).put(0,action.getJSONArray("value").getDouble(0)+size.getDouble(0)/2).put(1,action.getJSONArray("value").getDouble(1)+size.getDouble(1)/2);maskAction(JSONObject().put("kind","set").put("mask",maskId).put("property",JSONObject().put("node",action.getLong("node"))).put("frame",action.getInt("frame")).put("value",g),save,objectId)}
    }
}
