package com.motionstudio.editor

import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Replace only moov metadata; existing media offsets and compressed samples stay intact. */
internal object Mp4AudioTiming {
    private data class Box(val type:String,val data:ByteArray)
    private fun boxes(bytes:ByteArray):List<Box> {
        val b=ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN);val result=ArrayList<Box>()
        while(b.remaining()>0) {
            check(b.remaining()>=8){"MP4 元数据不完整"}
            val size=b.int.toLong() and 0xffffffffL;val type=ByteArray(4).apply{b.get(this)}.toString(Charsets.US_ASCII)
            check(size in 8..(b.remaining().toLong()+8)){"MP4 元数据尺寸错误"}
            result.add(Box(type,ByteArray(size.toInt()-8).apply{b.get(this)}))
        }
        return result
    }
    private fun box(type:String,data:ByteArray)=ByteBuffer.allocate(data.size+8).order(ByteOrder.BIG_ENDIAN).putInt(data.size+8).put(type.toByteArray(Charsets.US_ASCII)).put(data).array()
    private fun join(parts:List<Box>)=parts.fold(java.io.ByteArrayOutputStream()){out,p->out.apply{write(box(p.type,p.data))}}.toByteArray()
    private fun fullHeader(data:ByteArray,kind:String):Pair<Int,Int> {
        val version=data[0].toInt() and 255;check(version in 0..1){"MP4 时间格式未支持"}
        return when(kind){"tkhd"->(if(version==0)20 else 28) to (if(version==0)4 else 8);else->(if(version==0)16 else 24) to (if(version==0)4 else 8)}
    }
    private fun duration(data:ByteArray,kind:String,ticks:Long):ByteArray {
        val result=data.copyOf();val (offset,length)=fullHeader(data,kind);val b=ByteBuffer.wrap(result).order(ByteOrder.BIG_ENDIAN)
        if(length==4){check(ticks in 0..0xffffffffL){"MP4 时长超出范围"};b.putInt(offset,ticks.toInt())}else b.putLong(offset,ticks)
        return result
    }
    fun trim(file:File,delay:Int,samples:Long) {
        RandomAccessFile(file,"rw").use{f->
            var at=0L;var oldMoov=-1L;var parts:List<Box>?=null
            while(at<f.length()) {
                f.seek(at);val size=f.readInt().toLong() and 0xffffffffL;val name=ByteArray(4).apply{f.readFully(this)}.toString(Charsets.US_ASCII)
                val extended=if(size==1L)f.readLong()else if(size==0L)f.length()-at else size
                val header=if(size==1L)16 else 8
                check(extended>=header&&at+extended<=f.length()){"MP4 文件边界错误"}
                if(name=="moov"){
                    check(extended-header<=16L*1024*1024){"MP4 元数据超过预算"}
                    oldMoov=at;parts=boxes(ByteArray((extended-header).toInt()).apply{f.readFully(this)});break
                }
                at+=extended
            }
            val original=parts?:error("MP4 缺少时间信息")
            val movie=original.first{it.type=="mvhd"}.data
            val movieScale=ByteBuffer.wrap(movie).order(ByteOrder.BIG_ENDIAN).getInt(if(movie[0].toInt()==0)12 else 20).toLong() and 0xffffffffL
            val movieTicks=(samples*movieScale+24000)/48000
            var audioTracks=0
            val updated=original.map{part->when(part.type){
                "mvhd"->Box(part.type,duration(part.data,"mvhd",movieTicks))
                "trak"->{
                    val track=boxes(part.data)
                    val mdia=track.first{it.type=="mdia"};val media=boxes(mdia.data)
                    val handler=media.first{it.type=="hdlr"}.data
                    if(handler.copyOfRange(8,12).toString(Charsets.US_ASCII)!="soun")part else {
                        audioTracks++
                        val mdhd=media.first{it.type=="mdhd"}.data
                        val scale=ByteBuffer.wrap(mdhd).order(ByteOrder.BIG_ENDIAN).getInt(if(mdhd[0].toInt()==0)12 else 20).toLong() and 0xffffffffL
                        val edit=ByteBuffer.allocate(28).order(ByteOrder.BIG_ENDIAN).putInt(0x01000000).putInt(1).putLong(movieTicks).putLong(delay.toLong()*scale/48000).putShort(1).putShort(0).array()
                        val edts=Box("edts",box("elst",edit))
                        Box("trak",join(track.filter{it.type!="edts"}.map{if(it.type=="tkhd")Box("tkhd",duration(it.data,"tkhd",movieTicks))else it}+edts))
                    }
                }
                else->part
            }}
            check(audioTracks==1){"MP4 音轨数量错误"}
            val replacement=box("moov",join(updated))
            f.seek(f.length());f.write(replacement);f.fd.sync()
            f.seek(oldMoov+4);f.write("free".toByteArray(Charsets.US_ASCII));f.fd.sync()
        }
    }
}
