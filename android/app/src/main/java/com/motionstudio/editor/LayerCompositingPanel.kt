package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.JSONObject

private val blendChoices=listOf("normal" to "正常","add" to "相加","multiply" to "正片叠底","screen" to "滤色",
    "overlay" to "叠加","darken" to "变暗","lighten" to "变亮","difference" to "差值","exclusion" to "排除",
    "subtract" to "减去","divide" to "除以","color_dodge" to "颜色减淡","color_burn" to "颜色加深",
    "hard_light" to "强光","soft_light" to "柔光")
private val matteChoices=listOf("alpha" to "Alpha","alpha_inverted" to "反转 Alpha","luma" to "亮度","luma_inverted" to "反转亮度")

@Composable internal fun LayerCompositingPanel(vm:EditorViewModel,modifier:Modifier,backEnabled:Boolean=true) {
    val layer=vm.layer(vm.selected)?:return
    val blend=layer.optJSONObject("blend")
    val matte=layer.optJSONObject("track_matte")
    val sources=vm.state.project?.optJSONArray("layers").objects().filter{it.getLong("id")!=vm.selected&&it.getJSONObject("content").getString("kind") !in setOf("null","audio","adjustment")}
    fun setBlend(field:String,value:String){vm.edit(JSONObject().put("op","set_layer_blend").put("object",vm.selected).put(field,value))}
    fun setMatte(value:JSONObject?){vm.edit(JSONObject().put("op","set_track_matte").put("object",vm.selected).put("matte",value?:JSONObject.NULL))}
    BackHandler(enabled=backEnabled){vm.openProperty("position")}
    Column(modifier.background(Panel).testTag("layer-compositing-panel")) {
        Row(Modifier.fillMaxWidth().heightIn(min=56.dp),verticalAlignment=Alignment.CenterVertically) {
            Tool(Icons.AutoMirrored.Filled.ArrowBack,"返回变换"){vm.openProperty("position")}
            Text("图层混合",Modifier.weight(1f),color=Ink)
            Tool(Icons.Default.Close,"关闭混合面板"){vm.panelOpen=false}
        }
        HorizontalDivider(color=Muted.copy(alpha=.1f))
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal=12.dp,vertical=8.dp)) {
            CompositingChoice("混合模式","layer-blend-mode",blend?.optString("mode","normal")?:"normal",blendChoices,vm.editable()){setBlend("mode",it)}
            CompositingChoice("色彩空间","layer-blend-space",blend?.optString("space","linear")?:"linear",
                listOf("linear" to "线性","srgb" to "sRGB"),vm.editable()){setBlend("space",it)}
            Text("线性保留现有工程的混合方式；sRGB 可用于匹配非线性混合。",color=Muted,modifier=Modifier.padding(bottom=8.dp))
            val source=matte?.optLong("source",0)?.toString()?:"none"
            CompositingChoice("遮罩来源","layer-matte-source",source,listOf("none" to "无")+sources.map{it.getLong("id").toString() to it.getString("name")},vm.editable()) {
                if(it=="none")setMatte(null)else setMatte(JSONObject(matte?.toString()?:"{}").put("source",it.toLong())
                    .put("mode",matte?.optString("mode","alpha")?:"alpha").put("hide_source",matte?.optBoolean("hide_source",true)?:true))
            }
            if(matte!=null) {
                CompositingChoice("遮罩类型","layer-matte-mode",matte.optString("mode","alpha"),matteChoices,vm.editable()) {
                    setMatte(JSONObject(matte.toString()).put("mode",it))
                }
                Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                    Text("隐藏来源图层",Modifier.weight(1f),color=Ink)
                    Switch(matte.optBoolean("hide_source",true),{setMatte(JSONObject(matte.toString()).put("hide_source",it))},
                        enabled=vm.editable(),modifier=Modifier.testTag("layer-matte-hide-source"))
                }
                Text("遮罩读取来源的效果和不透明度，随图层变换与摄影机投影。",color=Muted)
            }
        }
    }
}

@Composable private fun CompositingChoice(label:String,tag:String,value:String,choices:List<Pair<String,String>>,enabled:Boolean,onSet:(String)->Unit) {
    var expanded by remember(tag){mutableStateOf(false)}
    Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
        Text(label,Modifier.weight(1f),color=Ink)
        Box(Modifier.weight(1.4f),contentAlignment=Alignment.CenterEnd) {
            TextButton(onClick={expanded=true},enabled=enabled,modifier=Modifier.heightIn(min=48.dp).testTag(tag)) {
                Text(choices.firstOrNull{it.first==value}?.second?:"来源不可用",maxLines=1,overflow=TextOverflow.Ellipsis)
            }
            DropdownMenu(expanded,{expanded=false}) {
                choices.forEach{(key,name)->DropdownMenuItem(text={Text(name)},modifier=Modifier.testTag("$tag-$key"),onClick={expanded=false;onSet(key)})}
            }
        }
    }
}
