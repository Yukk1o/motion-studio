package com.motionstudio.editor

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import java.io.File
import java.util.UUID

/** Import adapter only: preserves PNG originals and validates them off the UI
 * thread. Other Android formats retain the existing normalized PNG route. */
internal object ImageImport {
    data class Prepared(val file:File,val width:Int,val height:Int)
    fun prepare(resolver:ContentResolver,uri:Uri,root:File):Prepared {
        val file=File(root,"assets/${UUID.randomUUID()}.png")
        file.parentFile!!.mkdirs()
        var normalized:File?=null
        try {
            resolver.openInputStream(uri)?.use { input -> file.outputStream().use { output ->
                val buffer=ByteArray(64*1024);var bytes=0L
                while(true) {val count=input.read(buffer);if(count<0)break
                    bytes+=count;check(bytes<=64L*1024*1024){"图片文件超过 64 MiB"};output.write(buffer,0,count)}
            }}?:error("无法读取图片")
            val signature=ByteArray(8)
            val png=file.inputStream().use{it.read(signature)==8}&&signature.contentEquals(byteArrayOf(-119,80,78,71,13,10,26,10))
            if(!png) {
                val bitmap=ImageDecoder.decodeBitmap(ImageDecoder.createSource(file)){decoder,info,_->
                    check(info.size.width.toLong()*info.size.height*4<=64L*1024*1024){"非 PNG 图片超出解码预算，请转换为 PNG"}
                    decoder.allocator=ImageDecoder.ALLOCATOR_SOFTWARE
                }
                try {
                    normalized=File(root,"assets/${UUID.randomUUID()}.png")
                    normalized!!.outputStream().use{check(bitmap.compress(Bitmap.CompressFormat.PNG,100,it)){"图片转换失败"}}
                }finally{bitmap.recycle()}
                check(file.delete()){"图片临时文件清理失败"}
            }
            val original=normalized?:file
            val path="assets/"+original.name
            val info=nativeData(NativeBridge.prepareImage(root.absolutePath,path))
            check(info.getBoolean("validated")){"图片校验失败"}
            return Prepared(original,info.getInt("width"),info.getInt("height"))
        }catch(error:Throwable){file.delete();normalized?.delete();throw error}
    }
}
