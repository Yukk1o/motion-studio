package com.motionstudio.editor

import androidx.compose.runtime.*
import org.json.JSONObject

/** One pending value, one request in flight; the page keeps ownership of its undo gesture. */
internal class NativePluginColors(private val vm:EditorViewModel,private val host:PluginEditorHost,private val token:String) {
    var palette by mutableStateOf<ColorEditingSession?>(null);private set
    var busy by mutableStateOf(false);private set
    var error by mutableStateOf<String?>(null);private set
    private var param=""
    private var sending=false
    private var pending:Rgba?=null
    private var finishRequest:Pair<Boolean,()->Unit>?=null

    private fun request(value:JSONObject,done:(JSONObject)->Unit) {
        if(host.session?.token!=token||vm.isClosed)return
        val current=host.state?:return
        sending=true;busy=true;value.put("revision",current.getLong("revision"))
        host.request(value) reply@{reply->
            sending=false;busy=false
            if(host.session?.token!=token||vm.isClosed)return@reply
            if(!reply.optBoolean("ok")) {
                error=reply.optString("error","颜色编辑失败");pending=null;finishRequest=null
                host.state?.getJSONObject("values")?.optJSONArray(param)?.let{palette?.value=Rgba.from(it)}
            } else {error=null;done(reply.getJSONObject("result"))}
        }
    }
    fun open(definition:JSONObject,advanced:Boolean=false,onOpen:()->Unit={}) {
        if(busy||palette!=null||host.session?.token!=token)return
        vm.pause();param=definition.getString("id")
        request(JSONObject().put("op","color_begin").put("param",param)){state->
            palette=ColorEditingSession(definition.getString("name"),"native-param-$param",Rgba.from(state.getJSONObject("values").getJSONArray(param)),
                vm.root,vm.compositionId,state.getInt("frame"),true,definition.getDouble("min")..definition.getDouble("max"),{_,_->}).also{it.advanced=advanced}
            onOpen()
        }
    }
    fun preview(value:Rgba) {
        val current=palette?:return
        if(finishRequest!=null||(0..3).any{value.component(it) !in current.range})return
        current.value=value;pending=value;drain()
    }
    fun finish(commit:Boolean,after:()->Unit={}) {
        if(palette==null){after();return}
        if(finishRequest!=null){if(finishRequest?.first==commit)finishRequest=commit to after;return}
        vm.cancelEyedropper();finishRequest=commit to after
        if(!commit)pending=null
        drain()
    }
    private fun drain() {
        if(sending)return
        val value=pending
        if(value!=null) {
            pending=null
            request(JSONObject().put("op","set").put("param",param).put("value",value.array())){drain()}
        } else finishRequest?.let{(commit,_)->
            request(JSONObject().put("op","color_finish").put("commit",commit)){
                val after=finishRequest?.second?:{}
                finishRequest=null;palette=null;after()
            }
        }
    }
    fun pick(accept:(Rgba?)->Unit) {
        val current=palette?:return
        vm.beginEyedropper{sample->if(host.session?.token==token&&!vm.isClosed&&palette===current)accept(sample)}
    }
    fun pick()=pick{sample->if(sample!=null)palette?.value?.let{preview(it.copy(r=sample.r,g=sample.g,b=sample.b))}}
    fun choose(definition:JSONObject,value:Rgba)=open(definition){preview(value);finish(true)}
}
