package com.motionstudio.editor

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

@Composable internal fun HomeSettings(vm:EditorViewModel,layout:EditorLayoutState,onBack:()->Unit,onPackages:()->Unit,onAdjustLayout:()->Unit,onReport:()->Unit) {
    SettingsPage("设置","返回主页","home-settings-page",onBack) {tablet->
        val plugins:@Composable ()->Unit={
            SettingsSection("插件") {
                SettingsRow("效果包","安装、启用和卸载效果包","open-plugins",onClick=onPackages)
            }
        }
        val editorLayout:@Composable ()->Unit={
            SettingsSection("编辑器布局") {
                SettingsRow("调整布局",if(vm.state.project==null)"打开工程后可调整布局"else"调整预览、时间轴和属性面板的比例",
                    "adjust-layout",enabled=vm.state.project!=null&&!vm.state.busy,onClick=onAdjustLayout)
                HorizontalDivider(color=Muted.copy(alpha=.12f),modifier=Modifier.padding(horizontal=16.dp))
                SettingsRow("恢复默认布局","重置手机、平板和不同方向的布局比例","reset-layout",enabled=layout.customized,onClick=layout::reset)
            }
        }
        if(tablet)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(24.dp)) {
            Box(Modifier.weight(1f)){plugins()};Box(Modifier.weight(1f)){editorLayout()}
        }else Column(verticalArrangement=Arrangement.spacedBy(24.dp)){plugins();editorLayout()}
        Spacer(Modifier.height(24.dp))
        SettingsSection("问题诊断") {
            SettingsRow("导出错误报告","设备信息、错误原因和近期操作；不包含项目与素材","settings-error-report",onClick=onReport)
        }
    }
}

@Composable private fun SettingsSection(title:String,content:@Composable ColumnScope.()->Unit) {
    Column(Modifier.fillMaxWidth(),verticalArrangement=Arrangement.spacedBy(8.dp)) {
        Text(title,Modifier.padding(horizontal=4.dp),color=Muted,fontSize=13.sp)
        Surface(color=Panel,shape=RoundedCornerShape(12.dp),modifier=Modifier.fillMaxWidth()) {
            Column(content=content)
        }
    }
}

@Composable private fun SettingsRow(title:String,description:String,tag:String,enabled:Boolean=true,onClick:()->Unit) {
    Surface(onClick=onClick,enabled=enabled,color=Panel,modifier=Modifier.fillMaxWidth().testTag(tag)) {
        Row(Modifier.fillMaxWidth().heightIn(min=80.dp).padding(16.dp),verticalAlignment=Alignment.CenterVertically,
            horizontalArrangement=Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(6.dp)) {
                Text(title,color=if(enabled)Ink else Muted,fontSize=16.sp)
                Text(description,color=Muted,fontSize=13.sp,lineHeight=19.sp)
            }
            Icon(editorIcon(Icons.Default.ArrowDropDown),null,Modifier.size(18.dp).rotate(-90f),tint=Muted)
        }
    }
}

@Composable private fun SettingsPage(title:String,backLabel:String,tag:String,onBack:()->Unit,
    actions:@Composable RowScope.()->Unit={},content:@Composable (Boolean)->Unit) {
    Surface(color=Background,modifier=Modifier.fillMaxSize().testTag(tag)) {
        BoxWithConstraints(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.systemBars)) {
            val tablet=maxWidth>=600.dp
            val padding=if(tablet)24.dp else 16.dp
            Column(Modifier.align(Alignment.TopCenter).widthIn(max=1000.dp).fillMaxSize().padding(horizontal=padding)) {
                Row(Modifier.fillMaxWidth().heightIn(min=72.dp),verticalAlignment=Alignment.CenterVertically) {
                    Tool(Icons.AutoMirrored.Filled.ArrowBack,backLabel,action=onBack)
                    Text(title,Modifier.weight(1f).padding(start=8.dp),fontSize=22.sp,color=Ink)
                    actions()
                }
                Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(top=8.dp,bottom=24.dp)) {
                    content(tablet)
                }
            }
        }
    }
}

@Composable internal fun PluginSettings(vm:EditorViewModel,onInstall:()->Unit,onBack:()->Unit) {
    var remove by remember{mutableStateOf<JSONObject?>(null)}
    LaunchedEffect(Unit){vm.refreshCatalogue()}
    val available=vm.state.project!=null&&!vm.state.busy
    SettingsPage("效果包","返回设置","plugins-page",onBack,actions={
        TextButton(onClick=onInstall,enabled=available,modifier=Modifier.heightIn(min=48.dp).testTag("plugin-install")){Text("安装")}
    }) {
        Text("支持 .msfx 效果包。工程使用固定版本，分享工程时需提供对应效果包。",color=Muted,fontSize=13.sp,lineHeight=20.sp)
        Spacer(Modifier.height(20.dp))
        if(vm.state.busy||vm.catalogue==null) {
            LinearProgressIndicator(modifier=Modifier.fillMaxWidth().testTag("plugin-loading"))
            Spacer(Modifier.height(16.dp))
        }
        Surface(color=Panel,shape=RoundedCornerShape(12.dp),modifier=Modifier.fillMaxWidth()) {
            Column {
                effectPackages(vm).forEachIndexed{index,pkg->val manifest=pkg.getJSONObject("manifest")
                    val id=manifest.getString("id");val version=manifest.getString("version");val name=manifest.getString("name")
                    if(index>0)HorizontalDivider(color=Muted.copy(alpha=.12f),modifier=Modifier.padding(horizontal=16.dp))
                    Row(Modifier.fillMaxWidth().heightIn(min=88.dp).padding(horizontal=16.dp,vertical=8.dp),verticalAlignment=Alignment.CenterVertically) {
                        Column(Modifier.weight(1f).padding(end=12.dp),verticalArrangement=Arrangement.spacedBy(6.dp)) {
                            Text(name,color=Ink,fontSize=16.sp)
                            Text(version,color=Muted,fontSize=13.sp)
                            if(id!="com.motionstudio.effects.ae2021")TextButton(onClick={remove=pkg},enabled=available,
                                contentPadding=PaddingValues(horizontal=0.dp),modifier=Modifier.heightIn(min=48.dp).testTag("plugin-uninstall-$id-$version")){Text("卸载")}
                        }
                        Switch(checked=pkg.getBoolean("enabled"),onCheckedChange={on->vm.pluginOperation(JSONObject().put("op","enable").put("plugin",id).put("version",version).put("hash",pkg.getString("hash")).put("enabled",on))},
                            enabled=available,colors=SwitchDefaults.colors(checkedThumbColor=Background,checkedTrackColor=Accent,uncheckedThumbColor=Muted,uncheckedTrackColor=Panel),
                            modifier=Modifier.testTag("plugin-enable-$id-$version").semantics{contentDescription="$name $version 启用"})
                    }
                }
            }
        }
        vm.catalogue?.optJSONArray("errors")?.let{errors->
            for(i in 0 until errors.length())Text(errors.getString(i),Modifier.padding(top=12.dp),color=MaterialTheme.colorScheme.error,fontSize=13.sp)
        }
    }
    remove?.let{pkg->AlertDialog(onDismissRequest={remove=null},title={Text("卸载效果包")},
        text={Text("使用此固定版本的效果将显示缺失，工程中的参数仍保留。")},
        confirmButton={TextButton(onClick={val m=pkg.getJSONObject("manifest");vm.pluginOperation(JSONObject().put("op","uninstall").put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")));remove=null},enabled=available){Text("卸载")}},
        dismissButton={TextButton(onClick={remove=null}){Text("取消")}})}
}
