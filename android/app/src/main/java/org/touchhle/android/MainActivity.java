package org.touchhle.android;

import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.util.Log;

import org.libsdl.app.SDLActivity;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

public class MainActivity extends SDLActivity {
    private static final String TAG = "touchHLE";
    private static final int GAME_FOLDER_REQUEST = 4711;

    @Override
    protected String[] getLibraries() {
        return new String[]{
            "SDL2",
            "touchHLE"
        };
    }

    private static File gameFolderTarget() {
        return new File(getContext().getExternalFilesDir(null), "touchHLE_apps");
    }

    private static void copySelectedFolder(Uri treeUri) {
        File target = gameFolderTarget();
        if (!target.exists() && !target.mkdirs()) {
            Log.e(TAG, "Couldn't create game folder: " + target);
            return;
        }
        String documentId = DocumentsContract.getTreeDocumentId(treeUri);
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId);
        copyDocumentChildren(childrenUri, treeUri, target);
    }

    private static void copyDocumentChildren(Uri childrenUri, Uri treeUri, File target) {
        String[] projection = {
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE
        };
        try (Cursor cursor = getContext().getContentResolver().query(childrenUri, projection, null, null, null)) {
            if (cursor == null) return;
            int idColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID);
            int nameColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME);
            int mimeColumn = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE);
            while (cursor.moveToNext()) {
                String documentId = cursor.getString(idColumn);
                String name = cursor.getString(nameColumn);
                String mimeType = cursor.getString(mimeColumn);
                if (name == null || name.isEmpty() || name.equals(".") || name.equals("..")) continue;
                File destination = new File(target, name);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mimeType)) {
                    if (destination.exists() || destination.mkdirs()) {
                        Uri childUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId);
                        copyDocumentChildren(childUri, treeUri, destination);
                    }
                } else {
                    copyDocument(treeUri, documentId, destination);
                }
            }
        } catch (Exception ex) {
            Log.e(TAG, "Couldn't read selected game folder", ex);
        }
    }

    private static void copyDocument(Uri treeUri, String documentId, File destination) {
        Uri documentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, documentId);
        try (InputStream input = getContext().getContentResolver().openInputStream(documentUri);
             OutputStream output = new FileOutputStream(destination)) {
            if (input == null) return;
            byte[] buffer = new byte[1024 * 1024];
            int count;
            while ((count = input.read(buffer)) != -1) output.write(buffer, 0, count);
        } catch (Exception ex) {
            Log.e(TAG, "Couldn't copy selected game file: " + destination, ex);
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != GAME_FOLDER_REQUEST || resultCode != RESULT_OK || data == null || data.getData() == null) return;
        Uri treeUri = data.getData();
        try {
            getContentResolver().takePersistableUriPermission(treeUri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (Exception ignored) {
        }
        copySelectedFolder(treeUri);
    }

    public static int openURL(String url) {
        try {
            Uri uri = Uri.parse(url);
            if ("touchhle".equalsIgnoreCase(uri.getScheme()) && "game-folder".equalsIgnoreCase(uri.getHost())) {
                Intent picker = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
                picker.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                    | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                    | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
                mSingleton.startActivityForResult(picker, GAME_FOLDER_REQUEST);
                return 0;
            }
            return SDLActivity.openURL(url);
        } catch (Exception ex) {
            Log.e(TAG, "Couldn't open URL: " + url, ex);
            return -1;
        }
    }
}
