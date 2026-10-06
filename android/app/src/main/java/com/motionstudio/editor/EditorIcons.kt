package com.motionstudio.editor

import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.vectorResource

/** The SVG sources generate native paths; no bitmap rasterization is involved. */
@Composable internal fun editorIcon(icon:ImageVector):ImageVector {
    val resource=when(icon.name.substringAfterLast('.')) {
        "ArrowBack"->R.drawable.ic_editor_back
        "Close"->R.drawable.ic_editor_close
        "MoreHoriz"->R.drawable.ic_editor_more
        "MoreVert"->R.drawable.ic_editor_more_vertical
        "Undo"->R.drawable.ic_editor_undo
        "Redo"->R.drawable.ic_editor_redo
        "OpenWith"->R.drawable.ic_editor_move
        "RotateRight"->R.drawable.ic_editor_rotate
        "OpenInFull"->R.drawable.ic_editor_scale
        "Tune","Settings"->R.drawable.ic_editor_tune
        "IosShare"->R.drawable.ic_editor_share
        "PlayArrow"->R.drawable.ic_editor_play
        "Pause"->R.drawable.ic_editor_pause
        "SkipPrevious"->R.drawable.ic_editor_previous
        "SkipNext"->R.drawable.ic_editor_next
        "ShowChart"->R.drawable.ic_editor_curve
        "Link"->R.drawable.ic_editor_link
        "LinkOff"->R.drawable.ic_editor_unlink
        "Diamond"->R.drawable.ic_editor_diamond
        "Videocam"->R.drawable.ic_editor_camera
        "Image"->R.drawable.ic_editor_image
        "Movie"->R.drawable.ic_editor_movie
        "Audiotrack"->R.drawable.ic_editor_audio
        "TextFields"->R.drawable.ic_editor_text
        "Rectangle"->R.drawable.ic_editor_rectangle
        "ControlCamera"->R.drawable.ic_editor_controller
        "ArrowDropDown"->R.drawable.ic_editor_chevron_down
        "ContentCopy"->R.drawable.ic_editor_copy
        "ContentPaste"->R.drawable.ic_editor_paste
        "ContentCut"->R.drawable.ic_editor_cut
        "Search"->R.drawable.ic_editor_search
        "Check"->R.drawable.ic_editor_check
        "CropFree"->R.drawable.ic_editor_observe
        "Add"->R.drawable.ic_editor_add
        else->null
    }
    return resource?.let{ImageVector.vectorResource(it)}?:icon
}
