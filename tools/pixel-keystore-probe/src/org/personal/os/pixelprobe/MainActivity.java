package org.personal.os.pixelprobe;

import android.app.Activity;
import android.os.Bundle;
import android.content.pm.PackageManager;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;
import android.security.keystore.StrongBoxUnavailableException;
import android.util.Log;
import android.widget.TextView;

import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.Arrays;
import javax.crypto.AEADBadTagException;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.GCMParameterSpec;

public final class MainActivity extends Activity {
    private static final String TAG = "PixelKeyProbe";
    private static final String PROVIDER = "AndroidKeyStore";
    private static final String ALIAS = "temporary-personal-os-strongbox-probe";

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView output = new TextView(this);
        output.setText("Checking Android StrongBox; this may take a few seconds.");
        output.setTextSize(18);
        output.setPadding(32, 32, 32, 32);
        setContentView(output);

        String runId = getIntent().getStringExtra("run_id");
        new Thread(() -> runProbe(output, runId == null ? "manual" : runId)).start();
    }

    private void runProbe(TextView output, String runId) {
        boolean feature = getPackageManager().hasSystemFeature(
                PackageManager.FEATURE_STRONGBOX_KEYSTORE);
        KeyStore keyStore = null;
        String result;
        boolean roundTrip = false;
        boolean aadRoundTrip = false;
        boolean wrongAadRejected = false;
        boolean freshIvSample = false;
        int ivBytes = 0;
        int tagBytes = 0;
        String securityLevel = "NOT_TESTED";
        try {
            keyStore = KeyStore.getInstance(PROVIDER);
            keyStore.load(null);
            keyStore.deleteEntry(ALIAS);
            if (!feature) {
                result = "StrongBox feature is not declared";
            } else {
                KeyGenerator generator = KeyGenerator.getInstance(
                        KeyProperties.KEY_ALGORITHM_AES, PROVIDER);
                KeyGenParameterSpec specification = new KeyGenParameterSpec.Builder(
                        ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256)
                        .setRandomizedEncryptionRequired(true)
                        .setIsStrongBoxBacked(true)
                        .build();
                generator.init(specification);
                SecretKey key = generator.generateKey();

                SecretKeyFactory factory = SecretKeyFactory.getInstance(
                        key.getAlgorithm(), PROVIDER);
                KeyInfo info = (KeyInfo) factory.getKeySpec(key, KeyInfo.class);
                securityLevel = securityLevelName(info.getSecurityLevel());

                byte[] plaintext = "personal-os-strongbox-probe"
                        .getBytes(StandardCharsets.UTF_8);
                byte[] aad = "PVLTAD02|namespace=1|generation=42"
                        .getBytes(StandardCharsets.US_ASCII);
                Cipher encrypt = Cipher.getInstance("AES/GCM/NoPadding");
                encrypt.init(Cipher.ENCRYPT_MODE, key);
                encrypt.updateAAD(aad);
                byte[] ciphertext = encrypt.doFinal(plaintext);
                byte[] nonce = encrypt.getIV();
                ivBytes = nonce == null ? 0 : nonce.length;
                tagBytes = ciphertext.length - plaintext.length;

                Cipher encryptAgain = Cipher.getInstance("AES/GCM/NoPadding");
                encryptAgain.init(Cipher.ENCRYPT_MODE, key);
                encryptAgain.updateAAD(aad);
                encryptAgain.doFinal(plaintext);
                byte[] secondNonce = encryptAgain.getIV();
                freshIvSample = secondNonce != null
                        && secondNonce.length == 12
                        && !Arrays.equals(nonce, secondNonce);

                Cipher decrypt = Cipher.getInstance("AES/GCM/NoPadding");
                decrypt.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(128, nonce));
                decrypt.updateAAD(aad);
                roundTrip = Arrays.equals(plaintext, decrypt.doFinal(ciphertext));
                aadRoundTrip = roundTrip && ivBytes == 12 && tagBytes == 16;

                Cipher wrongAad = Cipher.getInstance("AES/GCM/NoPadding");
                wrongAad.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(128, nonce));
                wrongAad.updateAAD("wrong-associated-data".getBytes(StandardCharsets.US_ASCII));
                try {
                    wrongAad.doFinal(ciphertext);
                } catch (AEADBadTagException expected) {
                    wrongAadRejected = true;
                }
                result = aadRoundTrip && wrongAadRejected && freshIvSample
                        ? "AES-GCM provider contract passed"
                        : "AES-GCM provider contract failed";
            }
        } catch (StrongBoxUnavailableException error) {
            result = "StrongBox operation unavailable (" + error.getClass().getSimpleName() + ")";
        } catch (Exception error) {
            result = "Probe error (" + error.getClass().getSimpleName() + ")";
        }

        boolean aliasDeleted = false;
        if (keyStore != null) {
            try {
                keyStore.deleteEntry(ALIAS);
                aliasDeleted = !keyStore.containsAlias(ALIAS);
            } catch (Exception ignored) {
                aliasDeleted = false;
            }
        }

        String status = "RESULT run_id=" + runId
                + " feature=" + feature
                + " security_level=" + securityLevel
                + " aes_gcm_round_trip=" + (roundTrip ? "PASS" : "FAIL")
                + " iv_bytes=" + ivBytes
                + " tag_bytes=" + tagBytes
                + " aad_round_trip=" + (aadRoundTrip ? "PASS" : "FAIL")
                + " wrong_aad_rejected=" + (wrongAadRejected ? "PASS" : "FAIL")
                + " fresh_iv_sample=" + (freshIvSample ? "PASS" : "FAIL")
                + " alias_cleanup=" + (aliasDeleted ? "PASS" : "FAIL")
                + " detail=" + result;
        Log.i(TAG, status);
        String display = status.replace("RESULT run_id=" + runId + " ", "");
        runOnUiThread(() -> output.setText(display));
    }

    private static String securityLevelName(int level) {
        if (level == KeyProperties.SECURITY_LEVEL_STRONGBOX) {
            return "STRONGBOX";
        }
        if (level == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT) {
            return "TRUSTED_ENVIRONMENT";
        }
        if (level == KeyProperties.SECURITY_LEVEL_SOFTWARE) {
            return "SOFTWARE";
        }
        return "OTHER_" + level;
    }
}
