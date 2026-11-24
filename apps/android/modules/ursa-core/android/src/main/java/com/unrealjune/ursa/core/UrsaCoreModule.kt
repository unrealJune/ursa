package com.unrealjune.ursa.core

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.provider.Settings
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.functions.Coroutine
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.ursa_ffi.AccountKeys
import uniffi.ursa_ffi.JoinListener
import uniffi.ursa_ffi.LibraryItem
import uniffi.ursa_ffi.OfferListener
import uniffi.ursa_ffi.PoolNode
import uniffi.ursa_ffi.ProgressListener
import uniffi.ursa_ffi.Status
import uniffi.ursa_ffi.UploadRequest
import uniffi.ursa_ffi.UrsaCore
import uniffi.ursa_ffi.UrsaException
import java.io.File

/** A core failure, with the core's own message for the screen. */
class UrsaCoreException(message: String) : CodedException("ERR_URSA", message, null)

/**
 * The Ursa core for the app: [UrsaCore] (Rust, through UniFFI) plus what only Android can do —
 * turning a content URI into a file the core can read, thumbnails, and the MediaStore.
 *
 * The account root never touches this module's storage: the app keeps it in the Keystore
 * (expo-secure-store) and hands it to [init].
 */
class UrsaCoreModule : Module() {
  private val core: UrsaCore by lazy { UrsaCore() }

  private val context: Context
    get() = appContext.reactContext ?: throw UrsaCoreException("the app context is gone")

  /** Turns core errors into JS errors carrying the core's message. */
  private suspend fun <T> call(block: suspend () -> T): T =
    try {
      block()
    } catch (e: UrsaException) {
      throw UrsaCoreException(e.message ?: "Ursa failed")
    }

  private fun progress(task: String) =
    object : ProgressListener {
      override fun onProgress(done: ULong, total: ULong) {
        sendEvent("onProgress", mapOf("task" to task, "done" to done.toDouble(), "total" to total.toDouble()))
      }
    }

  private fun deviceName(): String =
    Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)?.takeIf { it.isNotBlank() }
      ?: "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}"

  override fun definition() = ModuleDefinition {
    Name("UrsaCore")

    Events("onProgress", "onLinkCode", "onLinkRequest", "onLinkConfirm")

    AsyncFunction("init") Coroutine { accountKey: String?, relays: List<String>, relayToken: String? ->
      val dir = File(context.filesDir, "ursa").apply { mkdirs() }
      call { core.init(dir.absolutePath, accountKey, relays, relayToken, deviceName(), "android").toMap() }
    }

    AsyncFunction("shutdown") Coroutine { -> call { core.shutdown() } }

    AsyncFunction("status") Coroutine { -> call { core.status().toMap() } }

    AsyncFunction("refresh") Coroutine { -> call { core.refresh().toMap() } }

    AsyncFunction("createAccount") Coroutine { name: String -> call { core.createAccount(name).toMap() } }

    AsyncFunction("recoverPhrase") Coroutine { phrase: String -> call { core.recoverPhrase(phrase).toMap() } }

    AsyncFunction("recoverKeyFile") Coroutine { uri: String, passphrase: String ->
      val json = withContext(Dispatchers.IO) {
        context.contentResolver.openInputStream(Uri.parse(uri))?.use { it.readBytes().decodeToString() }
          ?: throw UrsaCoreException("could not read the key file")
      }
      call { core.recoverKeyFile(json, passphrase).toMap() }
    }

    AsyncFunction("linkJoin") Coroutine { code: String ->
      val listener = object : JoinListener {
        override fun onConfirm(code: String) = sendEvent("onLinkConfirm", mapOf("code" to code))
      }
      call { core.linkJoin(code, listener).toMap() }
    }

    AsyncFunction("linkOffer") Coroutine { ->
      val listener = object : OfferListener {
        override fun onCode(code: String) = sendEvent("onLinkCode", mapOf("code" to code))

        override fun onRequest(name: String, platform: String, confirm: String) =
          sendEvent("onLinkRequest", mapOf("name" to name, "platform" to platform, "code" to confirm))
      }
      call { core.linkOffer(listener) }
    }

    AsyncFunction("linkDecide") Coroutine { accept: Boolean -> call { core.linkDecide(accept) } }

    AsyncFunction("linkCancel") Coroutine { -> call { core.linkCancel() } }

    AsyncFunction("pool") Coroutine { -> call { core.pool().map { it.toMap() } } }

    AsyncFunction("claimNode") Coroutine { link: String, label: String -> call { core.claimNode(link, label) } }

    AsyncFunction("forgetNode") Coroutine { nodeId: String -> call { core.forgetNode(nodeId) } }

    AsyncFunction("invite") Coroutine { -> call { core.invite() } }

    AsyncFunction("befriend") Coroutine { invite: String, quotaGb: Double ->
      call {
        val f = core.befriend(invite, quotaGb.toLong().toULong())
        mapOf("accountId" to f.accountId, "name" to f.name, "nodes" to f.nodes.toInt())
      }
    }

    AsyncFunction("recoveryPhrase") Coroutine { -> call { core.recoveryPhrase() } }

    /** Writes the key file into this app's cache and returns its path, for the share sheet. */
    AsyncFunction("exportKeyFile") Coroutine { passphrase: String ->
      val json = call { core.keyFile(passphrase) }
      withContext(Dispatchers.IO) {
        val dir = File(context.cacheDir, "export").apply { mkdirs() }
        val f = File(dir, "ursa-account.ursa-key")
        f.writeText(json)
        Uri.fromFile(f).toString()
      }
    }

    AsyncFunction("list") Coroutine { -> call { core.list().map { it.toMap() } } }

    AsyncFunction("thumb") Coroutine { fileId: String -> call { Uri.fromFile(File(core.thumb(fileId))).toString() } }

    /**
     * Uploads a file:// or content:// URI. A content URI is copied to the cache first, since
     * the core reads paths. `meta` may carry name, mime, createdMs, width and height.
     */
    AsyncFunction("upload") Coroutine { uri: String, meta: Map<String, Any?>, task: String ->
      val prepared = withContext(Dispatchers.IO) { Media.prepare(context, Uri.parse(uri), meta) }
      try {
        call {
          core.upload(
            UploadRequest(
              path = prepared.path,
              name = prepared.name,
              mime = prepared.mime,
              createdMs = prepared.createdMs,
              modifiedMs = prepared.modifiedMs,
              width = prepared.width?.toUInt(),
              height = prepared.height?.toUInt(),
              thumbPath = prepared.thumbPath,
            ),
            progress(task),
          ).toMap()
        }
      } finally {
        withContext(Dispatchers.IO) { prepared.cleanup() }
      }
    }

    /** Downloads into the cache (once) and returns the file's URI, for viewing or sharing. */
    AsyncFunction("download") Coroutine { fileId: String, name: String, task: String ->
      val dir = File(context.cacheDir, "files/$fileId").apply { mkdirs() }
      val out = File(dir, Media.safeName(name))
      if (!out.exists()) {
        call { core.download(fileId, out.absolutePath, progress(task)) }
      }
      Uri.fromFile(out).toString()
    }

    /** Copies a downloaded file into the shared gallery (or Downloads, for documents). */
    AsyncFunction("saveToDevice") Coroutine { fileUri: String, name: String, mime: String ->
      withContext(Dispatchers.IO) { Media.saveToDevice(context, File(Uri.parse(fileUri).path!!), name, mime) }
    }

    /** Opens a downloaded file in whatever app handles its type ("Open with"). */
    Function("open") { fileUri: String, mime: String ->
      val file = File(Uri.parse(fileUri).path!!)
      val uri = androidx.core.content.FileProvider.getUriForFile(context, "${context.packageName}.ursa.files", file)
      val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
        .setDataAndType(uri, mime.ifBlank { "*/*" })
        .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
      try {
        context.startActivity(android.content.Intent.createChooser(intent, null).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
      } catch (_: android.content.ActivityNotFoundException) {
        throw UrsaCoreException("No app on this phone opens ${mime.ifBlank { "this file" }}")
      }
    }

    AsyncFunction("delete") Coroutine { fileId: String ->
      call { core.delete(fileId) }
      File(context.cacheDir, "files/$fileId").deleteRecursively()
      Unit
    }

    AsyncFunction("repair") Coroutine { task: String ->
      call {
        val r = core.repair(progress(task))
        mapOf(
          "files" to r.files.toDouble(),
          "objectsChecked" to r.objectsChecked.toDouble(),
          "objectsRebuilt" to r.objectsRebuilt.toDouble(),
          "unrecoverable" to r.unrecoverable,
        )
      }
    }

    AsyncFunction("signOut") Coroutine { ->
      call { core.signOut() }
      File(context.cacheDir, "files").deleteRecursively()
      Unit
    }
  }
}

private fun AccountKeys.toMap() = mapOf("accountId" to accountId, "accountKey" to accountKey)

private fun Status.toMap() =
  mapOf(
    "deviceId" to deviceId,
    "hasAccount" to hasAccount,
    "accountId" to accountId,
    "name" to name,
    "openError" to openError,
    "devices" to devices.map {
      mapOf(
        "deviceId" to it.deviceId,
        "name" to it.name,
        "platform" to it.platform,
        "addedMs" to it.addedMs.toDouble(),
        "thisDevice" to it.thisDevice,
      )
    },
    "homeNodes" to homeNodes.map { mapOf("id" to it.id, "label" to it.label) },
    "friends" to friends.map { mapOf("accountId" to it.accountId, "name" to it.name, "nodes" to it.nodes.toInt()) },
  )

private fun LibraryItem.toMap() =
  mapOf(
    "fileId" to fileId,
    "name" to name,
    "size" to size.toDouble(),
    "mime" to mime,
    "createdMs" to createdMs.toDouble(),
    "modifiedMs" to modifiedMs.toDouble(),
    "addedMs" to addedMs.toDouble(),
    "width" to width?.toInt(),
    "height" to height?.toInt(),
    "hasThumb" to hasThumb,
  )

private fun PoolNode.toMap() =
  mapOf(
    "id" to id,
    "label" to label,
    "owner" to owner,
    "reachable" to reachable,
    "path" to path,
    "role" to role,
    "usedBytes" to usedBytes?.toDouble(),
    "quotaBytes" to quotaBytes?.toDouble(),
    "version" to version,
    "backend" to backend,
    "error" to error,
  )

/** What Android knows about a file that the core does not. */
internal object Media {
  class Prepared(
    val path: String,
    val name: String?,
    val mime: String?,
    val createdMs: Long?,
    val modifiedMs: Long?,
    val width: Int?,
    val height: Int?,
    val thumbPath: String?,
    private val temp: List<File>,
  ) {
    fun cleanup() = temp.forEach { it.delete() }
  }

  private const val THUMB_LONG_SIDE = 640

  fun safeName(name: String): String = name.replace(Regex("[/\\\\:*?\"<>|\\x00-\\x1f]"), "_").ifBlank { "file" }

  fun prepare(context: Context, uri: Uri, meta: Map<String, Any?>): Prepared {
    val temp = mutableListOf<File>()
    var name = meta["name"] as? String
    var mime = meta["mime"] as? String
    val file: File
    if (uri.scheme == "file") {
      file = File(uri.path!!)
      name = name ?: file.name
    } else {
      context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) name = name ?: c.getString(0)
      }
      mime = mime ?: context.contentResolver.getType(uri)
      val dir = File(context.cacheDir, "upload").apply { mkdirs() }
      file = File.createTempFile("up-", "-" + safeName(name ?: "file"), dir)
      temp += file
      context.contentResolver.openInputStream(uri)?.use { input -> file.outputStream().use { input.copyTo(it) } }
        ?: throw UrsaCoreException("could not read $uri")
    }
    if (mime == null) {
      mime = android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase())
    }
    var width = (meta["width"] as? Number)?.toInt()
    var height = (meta["height"] as? Number)?.toInt()
    val modified = (meta["modifiedMs"] as? Number)?.toLong() ?: file.lastModified().takeIf { it > 0 }
    var created = (meta["createdMs"] as? Number)?.toLong()

    var thumbPath: String? = null
    if (mime != null && (mime.startsWith("image/") || mime.startsWith("video/"))) {
      try {
        val size = android.util.Size(THUMB_LONG_SIDE, THUMB_LONG_SIDE)
        val bmp = if (mime.startsWith("image/")) {
          android.media.ThumbnailUtils.createImageThumbnail(file, size, null)
        } else {
          android.media.ThumbnailUtils.createVideoThumbnail(file, size, null)
        }
        val out = File(File(context.cacheDir, "upload").apply { mkdirs() }, "thumb-${System.nanoTime()}.jpg")
        out.outputStream().use { bmp.compress(android.graphics.Bitmap.CompressFormat.JPEG, 82, it) }
        bmp.recycle()
        temp += out
        thumbPath = out.absolutePath
      } catch (_: Exception) {
        // No thumbnail: the gallery shows a placeholder.
      }
      if (mime.startsWith("image/")) {
        if (width == null || height == null) {
          val o = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
          android.graphics.BitmapFactory.decodeFile(file.absolutePath, o)
          if (o.outWidth > 0) {
            width = o.outWidth
            height = o.outHeight
          }
        }
        if (created == null) created = exifTaken(file)
      } else if (width == null || height == null || created == null) {
        android.media.MediaMetadataRetriever().use { r ->
          try {
            r.setDataSource(file.absolutePath)
            val w = r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull()
            val h = r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull()
            val rot = r.extractMetadata(android.media.MediaMetadataRetriever.METADATA_KEY_VIDEO_ROTATION)?.toIntOrNull() ?: 0
            if (width == null && w != null && h != null) {
              width = if (rot % 180 == 0) w else h
              height = if (rot % 180 == 0) h else w
            }
          } catch (_: Exception) {
          }
        }
      }
    }
    return Prepared(file.absolutePath, name, mime, created ?: modified, modified, width, height, thumbPath, temp)
  }

  private fun exifTaken(file: File): Long? =
    try {
      val exif = android.media.ExifInterface(file.absolutePath)
      exif.dateTimeOriginal.takeIf { it > 0 } ?: exif.dateTime.takeIf { it > 0 }
    } catch (_: Exception) {
      null
    }

  fun saveToDevice(context: Context, file: File, name: String, mime: String): String {
    val resolver = context.contentResolver
    val (collection, dir) = when {
      mime.startsWith("image/") -> MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to "Pictures/Ursa"
      mime.startsWith("video/") -> MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to "Movies/Ursa"
      else -> MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to "Download/Ursa"
    }
    val values = ContentValues().apply {
      put(MediaStore.MediaColumns.DISPLAY_NAME, name)
      put(MediaStore.MediaColumns.MIME_TYPE, mime)
      put(MediaStore.MediaColumns.RELATIVE_PATH, dir)
      put(MediaStore.MediaColumns.IS_PENDING, 1)
    }
    val uri = resolver.insert(collection, values) ?: throw UrsaCoreException("could not save $name")
    resolver.openOutputStream(uri)?.use { out -> file.inputStream().use { it.copyTo(out) } }
    values.clear()
    values.put(MediaStore.MediaColumns.IS_PENDING, 0)
    resolver.update(uri, values, null, null)
    return uri.toString()
  }
}
