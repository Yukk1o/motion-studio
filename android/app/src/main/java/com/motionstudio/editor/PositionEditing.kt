package com.motionstudio.editor

import androidx.compose.runtime.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.floor

internal interface PositionEditActions {
    fun begin(target:JSONObject)
    fun value(value:JSONArray,axes:List<Int>,frame:Int?=null)
    fun spatial(frame:Int,tangents:JSONObject?)
    fun finish(commit:Boolean,after:()->Unit={})
}

internal fun isPositionParameter(desc:JSONObject)=desc.optString("kind") in listOf("vec2","vec3")&&
    (desc.optBoolean("center_default")||desc.optString("id") in listOf("position","center","origin","source_position"))

internal fun positionTarget(vm:EditorViewModel):JSONObject? {
    if(vm.property in listOf("position","target")&&vm.panelOpen)
        return JSONObject().put("kind","property").put("object",vm.selected).put("property",vm.property)
    val effect=effectTarget(vm.property)?:return null
    val instance=vm.layer(vm.selected)?.optJSONArray("effects").objects().firstOrNull{it.getLong("id")==effect.first}?:return null
    val desc=vm.effectDefinition(instance)?.optJSONArray("params").objects().firstOrNull{it.getString("id")==effect.second}?:return null
    return if(isPositionParameter(desc))JSONObject().put("kind","effect").put("object",vm.selected).put("effect",effect.first).put("param",effect.second)else null
}

internal class ProjectPositionActions(private val vm:EditorViewModel):PositionEditActions {
    private var target:JSONObject?=null
    private var frame=0
    override fun begin(target:JSONObject) {
        if(this.target!=null)return
        this.target=JSONObject(target.toString());frame=floor(vm.frame).toInt();vm.beginGesture()
    }
    override fun value(value:JSONArray,axes:List<Int>,frame:Int?) {
        val t=target?:return;val at=frame?:this.frame
        if(t.getString("kind")=="effect") {
            val padded=JSONArray(value.toString());while(padded.length()<4)padded.put(0)
            vm.effectAction(t.getLong("object"),t.getLong("effect"),"set",JSONObject().put("param",t.getString("param")).put("frame",at).put("value",padded),false)
        }else vm.setPropertyValue(t.getLong("object"),t.getString("property"),at,JSONArray(List(3){value.optDouble(it)}),false,axes)
    }
    override fun spatial(frame:Int,tangents:JSONObject?) {
        val t=target?:return
        if(t.getString("kind")=="effect")vm.effectAction(t.getLong("object"),t.getLong("effect"),"spatial",JSONObject().put("param",t.getString("param")).put("frame",frame).put("tangents",tangents?:JSONObject.NULL),false)
        else vm.edit(JSONObject(t.toString()).removeKind().put("op","spatial").put("frame",frame).put("tangents",tangents?:JSONObject.NULL),false)
    }
    override fun finish(commit:Boolean,after:()->Unit) {
        if(target!=null){if(commit)vm.endGesture(after)else{vm.cancelGesture();after()};target=null}else after()
    }
    private fun JSONObject.removeKind()=apply{remove("kind")}
}

/** A native page keeps its undo gesture; a drag rolls back only this vector track. */
internal class NativePluginPositions(private val host:PluginEditorHost):PositionEditActions {
    var active by mutableStateOf(false);private set
    var busy by mutableStateOf(false);private set
    var error by mutableStateOf<String?>(null);private set
    private var param=""
    private var pending:JSONObject?=null
    private var ending:Pair<Boolean,()->Unit>?=null
    private var opened=false
    override fun begin(target:JSONObject) {
        if(active||busy||host.session==null||target.optString("kind")!="effect")return
        param=target.getString("param");active=true;opened=false;error=null
        request(JSONObject().put("op","parameter_begin").put("param",param)){opened=true;drain()}
    }
    private fun request(command:JSONObject,done:()->Unit) {
        val state=host.state?:return
        busy=true;command.put("revision",state.getLong("revision"))
        host.request(command){reply->
            busy=false
            if(reply.optBoolean("ok"))done() else {
                error=reply.optString("error","位置编辑失败");pending=null
                if(!opened){active=false;ending?.second?.invoke();ending=null}
                else if(command.optString("op")!="parameter_finish") {
                    ending=false to (ending?.second?:{})
                    busy=true
                    host.request(JSONObject().put("op","state")){fresh->busy=false;if(fresh.optBoolean("ok"))drain()}
                }
            }
        }
    }
    override fun value(value:JSONArray,axes:List<Int>,frame:Int?) {
        if(!active||ending!=null)return
        val padded=JSONArray(value.toString());while(padded.length()<4)padded.put(0)
        pending=JSONObject().put("op",if(frame==null)"set"else"set_at").put("param",param).put("value",padded).apply{frame?.let{put("frame",it)}};drain()
    }
    override fun spatial(frame:Int,tangents:JSONObject?) {
        if(!active||ending!=null)return
        pending=JSONObject().put("op","spatial").put("param",param).put("frame",frame).put("tangents",tangents?:JSONObject.NULL);drain()
    }
    override fun finish(commit:Boolean,after:()->Unit) {
        if(!active){after();return}
        ending=commit to after;if(!commit)pending=null;drain()
    }
    private fun drain() {
        if(busy||!opened)return
        val next=pending
        if(next!=null){pending=null;request(next){drain()}}
        else ending?.let{(commit,_)->request(JSONObject().put("op","parameter_finish").put("commit",commit)){
            val after=ending?.second?:{};ending=null;active=false;opened=false;after()
        }}
    }
}
