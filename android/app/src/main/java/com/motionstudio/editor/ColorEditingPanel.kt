package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable internal fun PaletteSwatch(color:Rgba,label:String,tag:String,selected:Boolean=false,enabled:Boolean=true,onClick:()->Unit) {
    Surface(onClick=onClick,enabled=enabled,modifier=Modifier.size(48.dp).testTag(tag).semantics{contentDescription=label},color=Color.Transparent,shape=RoundedCornerShape(8.dp)) {
        Canvas(Modifier.fillMaxSize().padding(6.dp)){drawRoundRect(color.color(alpha=false),cornerRadius=androidx.compose.ui.geometry.CornerRadius(6.dp.toPx()));
            drawRoundRect(if(selected)Ink else Muted.copy(alpha=.25f),cornerRadius=androidx.compose.ui.geometry.CornerRadius(6.dp.toPx()),style=Stroke(if(selected)2.dp.toPx()else 1.dp.toPx()))}
    }
}

@Composable internal fun ColorEditingPanel(vm:EditorViewModel,modifier:Modifier) {
    val session=vm.colorEditor?:return
    ColorEditingPanel(session,modifier,vm::previewColor,vm::finishColorEditor,{accept->
        vm.beginEyedropper{sample->if(vm.colorEditor===session)accept(sample)}
    },vm.eyedropperActive)
}

@Composable internal fun ColorEditingPanel(session:ColorEditingSession,modifier:Modifier,onPreview:(Rgba)->Unit,
    onFinish:(Boolean)->Unit,onEyedropper:((Rgba?)->Unit)->Unit,eyedropperActive:Boolean) {
    val context=LocalContext.current
    val store=remember(context){ColorBookmarks(context)}
    var favorites by remember{mutableStateOf(store.list())}
    var add by remember{mutableStateOf(false)}
    var menu by remember{mutableStateOf<ColorBookmark?>(null)}
    var rename by remember{mutableStateOf<ColorBookmark?>(null)}
    var managing by remember{mutableStateOf(false)}
    var common by remember{mutableStateOf(store.commonIds())}
    fun allowed(value:Rgba)=(0 until if(session.alphaEditable)4 else 3).all{value.component(it) in session.range}
    LaunchedEffect(session.advanced){if(!session.advanced){favorites=store.list();common=store.commonIds()}}
    BackHandler {if(session.advanced)session.advanced=false else onFinish(false)}
    if(session.advanced) {
        key(session){AdvancedColorEditor(session.title,session.value,modifier,session.alphaEditable,session.range,
            onPreview=onPreview,onConfirm={onFinish(true)},onCancel={onFinish(false)},onBack={session.advanced=false},
            onEyedropper={accept->onEyedropper{sample->if(sample!=null)accept(sample)}})}
        return
    }
    Column(modifier.background(Panel).padding(horizontal=10.dp).testTag("color-selection-panel")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
            Text(session.title,Modifier.weight(1f),color=Ink,fontSize=15.sp)
            Text(session.value.hex(session.alphaEditable),color=Muted,fontSize=12.sp)
            TextButton(onClick={onFinish(false)},modifier=Modifier.heightIn(min=48.dp).testTag("color-cancel")){Text("取消")}
            TextButton(onClick={onFinish(true)},modifier=Modifier.heightIn(min=48.dp).testTag("color-confirm")){Text("完成")}
        }
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                TextButton(onClick={onEyedropper{sample->if(sample!=null)onPreview(session.value.copy(r=sample.r,g=sample.g,b=sample.b))}},
                    modifier=Modifier.heightIn(min=48.dp).testTag("color-eyedropper")){Text(if(eyedropperActive)"点选画面…"else"吸管")}
                TextButton(onClick={session.advanced=true},modifier=Modifier.heightIn(min=48.dp).testTag("color-more")){Text("调色盘")}
                TextButton(onClick={add=true},modifier=Modifier.heightIn(min=48.dp).testTag("color-bookmark-add")){Text("收藏当前颜色")}
                TextButton(onClick={managing=!managing},modifier=Modifier.heightIn(min=48.dp).testTag("color-common-manage")){Text(if(managing)"完成常用设置"else"设置常用色块")}
            }
            if(!managing&&common.isNotEmpty()) {
                Text("常用",color=Muted,fontSize=12.sp,modifier=Modifier.padding(top=8.dp))
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).testTag("color-common-colors")) {
                    store.common().forEach{(id,c)->
                        val chosen=if(id.startsWith("favorite:")&&session.alphaEditable)c else c.copy(a=session.value.a)
                        PaletteSwatch(c,c.hex(),"color-common-$id",session.value==chosen,allowed(chosen)){onPreview(chosen)}
                    }
                }
            }
            Text("内置",color=Muted,fontSize=12.sp,modifier=Modifier.padding(top=8.dp))
            BoxWithConstraints(Modifier.fillMaxWidth()) {
                val columns=(maxWidth.value/48).toInt().coerceAtLeast(1)
                Column {builtInColors.chunked(columns).forEach{row->Row {row.forEach{c->PaletteSwatch(c,c.hex(),"color-preset-${c.hex().drop(1)}",selected=if(managing)"builtin:${c.hex()}" in common else session.value.copy(a=1.0)==c,enabled=managing||allowed(c.copy(a=session.value.a))){
                    if(managing){store.toggleCommon("builtin:${c.hex()}");common=store.commonIds()}else onPreview(c.copy(a=session.value.a))
                }}}}}
            }
            Text("收藏",color=Muted,fontSize=12.sp,modifier=Modifier.padding(top=8.dp))
            if(favorites.isEmpty())Text("还没有收藏颜色",color=Muted,fontSize=12.sp,modifier=Modifier.padding(vertical=8.dp))
            else {
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {favorites.forEach{bookmark->
                    Column(horizontalAlignment=Alignment.CenterHorizontally) {
                        PaletteSwatch(bookmark.color,bookmark.name,"color-bookmark-${bookmark.id}",selected=managing&&"favorite:${bookmark.id}" in common,enabled=managing||allowed(if(session.alphaEditable)bookmark.color else bookmark.color.copy(a=session.value.a))){
                            if(managing){store.toggleCommon("favorite:${bookmark.id}");common=store.commonIds()}else onPreview(if(session.alphaEditable)bookmark.color else bookmark.color.copy(a=session.value.a))
                        }
                        TextButton(onClick={menu=bookmark},modifier=Modifier.heightIn(min=48.dp)){Text(bookmark.name.take(10),fontSize=11.sp)}
                    }
                }}
            }
            if(managing)Text("点选色块可加入或移出常用；下方列表决定显示顺序。",color=Muted,fontSize=12.sp,modifier=Modifier.padding(vertical=8.dp))
            if(managing)common.forEach{id->
                Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                    Text(if(id.startsWith("builtin:"))id.substringAfter(':')else favorites.firstOrNull{it.id==id.removePrefix("favorite:")}?.name?:"收藏颜色",Modifier.weight(1f),color=Muted,fontSize=12.sp)
                    TextButton(onClick={store.moveCommon(id,-1);common=store.commonIds()},enabled=common.indexOf(id)>0,modifier=Modifier.heightIn(min=48.dp).testTag("color-common-up-$id")){Text("前移")}
                    TextButton(onClick={store.moveCommon(id,1);common=store.commonIds()},enabled=common.indexOf(id)<common.lastIndex,modifier=Modifier.heightIn(min=48.dp).testTag("color-common-down-$id")){Text("后移")}
                    TextButton(onClick={store.toggleCommon(id);common=store.commonIds()},modifier=Modifier.heightIn(min=48.dp).testTag("color-common-remove-$id")){Text("移除")}
                }
            }
        }
    }
    if(add)InputDialog("收藏颜色",session.value.hex(),{add=false}){name->store.add(name,session.value);favorites=store.list();add=false}
    menu?.let{entry->AlertDialog(onDismissRequest={menu=null},title={Text(entry.name)},text={Text(entry.color.hex())},
        confirmButton={TextButton(onClick={menu=null;rename=entry}){Text("重命名")}},dismissButton={TextButton(onClick={store.remove(entry.id);favorites=store.list();common=store.commonIds();menu=null},modifier=Modifier.testTag("color-bookmark-delete")){Text("删除")}})}
    rename?.let{entry->InputDialog("收藏名称",entry.name,{rename=null}){name->store.rename(entry.id,name);favorites=store.list();rename=null}}
}
