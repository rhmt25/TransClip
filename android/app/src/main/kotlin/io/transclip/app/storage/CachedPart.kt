package io.transclip.app.storage

import io.transclip.engine.PartWriter
import java.io.File
import java.io.RandomAccessFile
import java.security.MessageDigest

/**
 * Part file in the app cache so an interrupted transfer can resume; [publish] moves the verified
 * bytes to their final place (MediaStore or a SAF tree) and returns the final file name.
 */
abstract class CachedPart(private val part: File) : PartWriter {
    private val file = RandomAccessFile(part, "rw").also { it.seek(it.length()) }
    final override var length: Long = file.length()
        private set

    override fun hashExisting(md: MessageDigest) {
        file.seek(0)
        val buf = ByteArray(256 * 1024)
        while (true) {
            val n = file.read(buf)
            if (n <= 0) break
            md.update(buf, 0, n)
        }
        file.seek(file.length())
    }

    override fun reset() {
        file.setLength(0)
        file.seek(0)
        length = 0
    }

    override fun write(data: ByteArray, off: Int, len: Int) {
        file.write(data, off, len)
        length += len
    }

    /** Copy the finished [part] to its destination; returns the final file name. */
    protected abstract fun publish(part: File, mtimeSecs: Long): String

    override fun finish(mtimeSecs: Long): String {
        file.close()
        val name = publish(part, mtimeSecs)
        part.delete()
        return name
    }

    override fun abort() {
        runCatching { file.close() }
        part.delete()
    }
}
