package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable internal fun LayerSelectionBar(vm:EditorViewModel) {
    var more by remember{mutableStateOf(false)}
    var deleting by remember{mutableStateOf(false)}
    var moving by remember{mutableStateOf(false)}
    var precompose by remember{mutableStateOf(false)}
    val layers=vm.selectedLayers();val editable=vm.selectionEditable()
    BackHandler{vm.finishLayerSelection()}
    Row(Modifier.fillMaxWidth().heightIn(min=52.dp).background(Panel).padding(horizontal=8.dp).testTag("layer-selection-bar"),verticalAlignment=Alignment.CenterVertically) {
        Tool(Icons.Default.Close,"退出多选",action=vm::finishLayerSelection)
        Text("已选 ${layers.size}",Modifier.weight(1f).testTag("selected-layer-count"),fontSize=14.sp,color=Accent)
        TextButton(onClick={if(layers.size==vm.state.project?.optJSONArray("layers")?.length())vm.clearLayerSelection()else vm.selectAllLayers()},modifier=Modifier.heightIn(min=48.dp).testTag("select-all-layers")) {
            Text(if(layers.isNotEmpty()&&layers.size==vm.state.project?.optJSONArray("layers")?.length())"清空"else"全选")
        }
        Box {
            Tool(Icons.Default.MoreHoriz,"批量图层操作",layers.isNotEmpty()){more=true}
            DropdownMenu(more,{more=false}) {
                val hidden=layers.all{!it.getBoolean("visible")};val locked=layers.all{it.getBoolean("locked")}
                DropdownMenuItem(text={Text(if(hidden)"显示所选"else"隐藏所选")},modifier=Modifier.testTag("selected-visibility"),onClick={more=false;vm.selectedFlags(visible=hidden)})
                DropdownMenuItem(text={Text(if(locked)"解锁所选"else"锁定所选")},modifier=Modifier.testTag("selected-lock"),onClick={more=false;vm.selectedFlags(locked=!locked)})
                DropdownMenuItem(text={Text("创建所选副本")},enabled=editable,modifier=Modifier.testTag("selected-duplicate"),onClick={more=false;vm.duplicateSelectedLayers()})
                DropdownMenuItem(text={Text("移动所选片段")},enabled=vm.selectedClipDeltaRange()!=null,modifier=Modifier.testTag("selected-move"),onClick={more=false;moving=true})
                DropdownMenuItem(text={Text("预合成所选图层")},enabled=vm.canPrecompose(),modifier=Modifier.testTag("selected-precompose"),onClick={more=false;precompose=true})
                DropdownMenuItem(text={Text("删除所选")},enabled=editable,modifier=Modifier.testTag("selected-delete"),onClick={more=false;deleting=true})
                if(!editable)DropdownMenuItem(text={Text("含锁定图层，解锁后可编辑",fontSize=12.sp)},enabled=false,onClick={})
            }
        }
    }
    if(precompose)InputDialog("预合成名称","预合成",{precompose=false}){vm.precompose(it);precompose=false}
    if(deleting)AlertDialog(onDismissRequest={deleting=false},title={Text("删除 ${layers.size} 个图层？")},text={Text("将同时移除这些图层的动画和效果。可以通过一次撤销恢复。")},
        confirmButton={TextButton(onClick={deleting=false;vm.deleteSelectedLayers()},enabled=editable,modifier=Modifier.testTag("confirm-selected-delete")){Text("删除")}},dismissButton={TextButton(onClick={deleting=false}){Text("取消")}})
    if(moving) {
        var value by remember{mutableStateOf("0")};var invalid by remember{mutableStateOf(false)}
        val range=vm.selectedClipDeltaRange()
        AlertDialog(onDismissRequest={moving=false},title={Text("移动所选片段")},text={Column {
            Text("保持图层之间的时间差。正数向后移动，负数向前移动。",color=Muted,fontSize=13.sp)
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(value,{value=it;invalid=false},label={Text("偏移帧数")},singleLine=true,isError=invalid,keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number),modifier=Modifier.fillMaxWidth().testTag("selected-move-offset"))
            TextButton(onClick={value=if(value.startsWith("-"))value.drop(1)else"-"+value}){Text("切换正负号")}
            Text(if(invalid)"输入超出可移动范围"else range?.let{"可移动范围：${it.first} 至 ${it.last} 帧"}?:"所选图层暂时无法移动",color=if(invalid)MaterialTheme.colorScheme.error else Muted,fontSize=12.sp)
        }},confirmButton={TextButton(onClick={val delta=value.toIntOrNull();if(delta!=null&&vm.moveSelectedClips(delta))moving=false else invalid=true},enabled=range!=null,modifier=Modifier.testTag("confirm-selected-move")){Text("移动")}},dismissButton={TextButton(onClick={moving=false}){Text("取消")}})
    }
}
