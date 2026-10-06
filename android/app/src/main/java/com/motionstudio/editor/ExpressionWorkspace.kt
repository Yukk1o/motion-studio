package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

/** An editor in the normal workspace, with the composition and timeline alongside it. */
@Composable internal fun ExpressionWorkspace(vm:EditorViewModel,modifier:Modifier,backEnabled:Boolean=true) {
    val initial=vm.expressionTarget?:return
    var axis by rememberSaveable(initial.toString()){mutableStateOf(initial.optString("axis",""))}
    val target=remember(initial.toString(),axis){JSONObject(initial.toString()).apply{remove("axis");if(axis.isNotEmpty())put("axis",axis)}}
    val vector=initial.optString("kind")=="property"&&initial.optString("property") in listOf("position","rotation","scale","target")
    BackHandler(enabled=backEnabled){vm.closeExpression()}
    Column(modifier.background(Panel).testTag("expression-workspace")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
            Tool(Icons.Default.ArrowBack,"返回属性",action=vm::closeExpression)
            Text("表达式",Modifier.weight(1f),color=Ink,fontSize=15.sp)
            Text(if(axis.isEmpty())"整个属性"else axis.uppercase(),Modifier.padding(end=12.dp),color=Muted,fontSize=12.sp)
        }
        if(vector)Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            (listOf("" to "整体")+vm.visibleAxes().map{listOf("x","y","z")[it] to listOf("X","Y","Z")[it]}).forEach{(key,label)->
                TextButton(onClick={axis=key},modifier=Modifier.height(48.dp).testTag("expression-scope-${key.ifEmpty{"all"}}")){Text(label,color=if(axis==key)Accent else Muted)}
            }
        }
        key(target.toString()) { ExpressionDraft(vm,target,Modifier.weight(1f).fillMaxWidth()) }
    }
}

@Composable private fun ExpressionDraft(vm:EditorViewModel,target:JSONObject,modifier:Modifier) {
    val existing=vm.expressionFor(target)
    var source by rememberSaveable{mutableStateOf(existing?.optString("source")?:"value")}
    var enabled by rememberSaveable{mutableStateOf(existing?.optBoolean("enabled",true)?:true)}
    var seed by rememberSaveable{mutableStateOf((existing?.optLong("seed",0)?:0).toString())}
    var error by remember{mutableStateOf<String?>(null)}
    var busy by remember{mutableStateOf(false)}
    val revision=vm.state.sample?.optLong("revision")
    // Undo and redo refresh saved source, but never overwrite an unapplied draft.
    var savedSource by remember{mutableStateOf(existing?.toString())}
    LaunchedEffect(revision) {
        val next=existing?.toString()
        if(next!=savedSource) {
            val old=savedSource?.let(::JSONObject)
            if(source==(old?.optString("source")?:"value")&&enabled==(old?.optBoolean("enabled",true)?:true)&&seed==(old?.optLong("seed",0)?:0).toString()) {
                source=existing?.optString("source")?:"value";enabled=existing?.optBoolean("enabled",true)?:true;seed=(existing?.optLong("seed",0)?:0).toString()
            }
            savedSource=next
        }
    }
    val conflicts=vm.state.project?.optJSONArray("expressions").objects().filter{record->
        val other=record.getJSONObject("target")
        target.optString("kind")=="property"&&other.optString("kind")=="property"&&
            other.optLong("object")==target.optLong("object")&&other.optString("property")==target.optString("property")&&
            other.has("axis")!=target.has("axis")
    }
    Column(modifier.verticalScroll(rememberScrollState()).imePadding().padding(horizontal=12.dp)) {
        Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
            Text("启用",Modifier.weight(1f),color=Ink)
            Switch(enabled,{enabled=it},enabled=vm.editable()&&!busy,modifier=Modifier.testTag("expression-enabled"))
        }
        Text("value 为原属性值；time 为秒。仅支持数值表达式及已提供的动画函数。",color=Muted,fontSize=12.sp)
        OutlinedTextField(source,{source=it;error=null},textStyle=TextStyle(fontFamily=FontFamily.Monospace,fontSize=14.sp),
            label={Text("表达式代码")},minLines=3,maxLines=8,readOnly=!vm.editable()||busy,
            modifier=Modifier.fillMaxWidth().testTag("expression-source"))
        OutlinedTextField(seed,{seed=it;error=null},label={Text("随机种子")},singleLine=true,
            keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number),enabled=vm.editable()&&!busy,
            modifier=Modifier.fillMaxWidth().testTag("expression-seed"))
        conflicts.forEach{record->
            val other=record.getJSONObject("target")
            Text(if(other.has("axis"))"已有 ${other.getString("axis").uppercase()} 分量表达式，移除后才能使用整体表达式。"else"已有整体表达式，移除后才能使用分量表达式。",color=Muted,fontSize=12.sp)
            TextButton(onClick={busy=true;vm.removeExpression(other){error=it;busy=false}},enabled=vm.editable()&&!busy,
                modifier=Modifier.height(48.dp).testTag("expression-remove-conflict")){Text("移除冲突表达式（可撤销）")}
        }
        error?.let{Text(it,color=MaterialTheme.colorScheme.error,fontSize=12.sp,modifier=Modifier.testTag("expression-error"))}
        Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
            TextButton(onClick={
                val number=seed.toLongOrNull()
                when {
                    source.toByteArray(Charsets.UTF_8).size>8192->error="代码超过 8192 字节"
                    number==null||number !in 0L..4294967295L->error="种子必须是 0～4294967295 的整数"
                    else->{busy=true;vm.saveExpression(target,source,enabled,number){error=it;busy=false}}
                }
            },enabled=vm.editable()&&!busy&&conflicts.isEmpty(),modifier=Modifier.height(48.dp).testTag("expression-apply")){Text(if(busy)"保存中…"else"应用")}
            if(existing!=null)TextButton(onClick={busy=true;vm.removeExpression(target){error=it;busy=false;if(it==null){source="value";enabled=true;seed="0"}}},
                enabled=vm.editable()&&!busy,modifier=Modifier.height(48.dp).testTag("expression-remove")){Text("移除")}
        }
        Text("编译或运行失败时保留代码和关键帧；关闭启用可保存草稿。",color=Muted,fontSize=12.sp)
        Spacer(Modifier.height(12.dp))
    }
}
