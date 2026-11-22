#include <errno.h>
#include <libusb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

#define PONGO_VID 0x05ac
#define PONGO_PID 0x4141
#define PONGO_EP_OUT 0x02
#define PONGO_USB_TIMEOUT_MS 5000
#define PONGO_UPLOAD_MAX_SIZE (128u * 1024u * 1024u)

static int control(libusb_device_handle *handle, uint8_t request_type,
                   uint8_t request, uint16_t value, uint16_t index,
                   unsigned char *data, uint16_t length)
{
    int result = libusb_control_transfer(handle, request_type, request, value,
                                         index, data, length,
                                         PONGO_USB_TIMEOUT_MS);
    if (result < 0) {
        fprintf(stderr, "USB control request %u failed: %s\n", request,
                libusb_error_name(result));
        return result;
    }
    if (result != length) {
        fprintf(stderr,
                "USB control request %u transferred %d of %u bytes\n",
                request, result, length);
        return LIBUSB_ERROR_IO;
    }
    return 0;
}

int main(int argc, char **argv)
{
    struct stat info;
    libusb_device **devices = NULL;
    libusb_device_handle *handle = NULL;
    unsigned char *image = NULL;
    size_t offset = 0;
    size_t image_size;
    ssize_t count;
    int result;
    int claimed = 0;
    FILE *file = NULL;
    char boot_command[] = "bootr\n";

    if (argc != 2) {
        fprintf(stderr, "usage: %s pongo_raw_probe.bin\n", argv[0]);
        return 2;
    }
    if (stat(argv[1], &info) != 0 || info.st_size <= 0 ||
        (uintmax_t)info.st_size > PONGO_UPLOAD_MAX_SIZE) {
        fprintf(stderr, "invalid or oversized probe image: %s\n", argv[1]);
        return 2;
    }
    image_size = (size_t)info.st_size;
    image = malloc(image_size);
    if (!image) {
        perror("malloc");
        return 1;
    }
    file = fopen(argv[1], "rb");
    if (!file) {
        perror("fopen");
        free(image);
        return 1;
    }
    if (fread(image, 1, image_size, file) != image_size) {
        fprintf(stderr, "short read from %s\n", argv[1]);
        fclose(file);
        free(image);
        return 1;
    }
    fclose(file);
    file = NULL;

    result = libusb_init(NULL);
    if (result < 0) {
        fprintf(stderr, "libusb_init: %s\n", libusb_error_name(result));
        free(image);
        return 1;
    }
    count = libusb_get_device_list(NULL, &devices);
    if (count < 0) {
        result = (int)count;
        fprintf(stderr, "libusb_get_device_list: %s\n",
                libusb_error_name(result));
        goto cleanup;
    }

    for (ssize_t i = 0; i < count; i++) {
        struct libusb_device_descriptor desc;
        if (libusb_get_device_descriptor(devices[i], &desc) == 0 &&
            desc.idVendor == PONGO_VID && desc.idProduct == PONGO_PID) {
            result = libusb_open(devices[i], &handle);
            if (result < 0) {
                fprintf(stderr, "libusb_open: %s\n",
                        libusb_error_name(result));
                goto cleanup;
            }
            break;
        }
    }
    libusb_free_device_list(devices, 1);
    devices = NULL;
    if (!handle) {
        fprintf(stderr, "Pongo USB device %04x:%04x not found\n",
                PONGO_VID, PONGO_PID);
        result = LIBUSB_ERROR_NO_DEVICE;
        goto cleanup;
    }

    libusb_set_auto_detach_kernel_driver(handle, 1);
    result = libusb_claim_interface(handle, 0);
    if (result < 0) {
        fprintf(stderr, "claim interface 0: %s\n", libusb_error_name(result));
        goto cleanup;
    }
    claimed = 1;

    /*
     * Pongo defaults to a 1 MiB OUT buffer. Request the full image-sized
     * receive allocation; on Pongo this request also clears prior data and
     * arms the bulk OUT transfer. Its allocator rounds the size to 512 bytes.
     */
    uint32_t requested_size = (uint32_t)image_size;
    if (control(handle, 0x21, 1, 0, 0,
                (unsigned char *)&requested_size,
                (uint16_t)sizeof(requested_size)) < 0) {
        result = LIBUSB_ERROR_IO;
        goto cleanup;
    }

    fprintf(stderr, "Uploading raw probe (%zu bytes)\n", image_size);
    while (offset < image_size) {
        int transferred = 0;
        int chunk = (int)(image_size - offset);
        if (chunk > 1024 * 1024) {
            chunk = 1024 * 1024;
        }
        result = libusb_bulk_transfer(handle, PONGO_EP_OUT, image + offset,
                                      chunk, &transferred,
                                      PONGO_USB_TIMEOUT_MS);
        if (result < 0 || transferred <= 0) {
            if (result >= 0) {
                result = LIBUSB_ERROR_IO;
            }
            fprintf(stderr, "bulk transfer at %zu/%zu failed: %s (%d bytes)\n",
                    offset, image_size,
                    result < 0 ? libusb_error_name(result) : "no progress",
                    transferred);
            goto cleanup;
        }
        offset += (size_t)transferred;
    }
    fprintf(stderr, "Transfer complete; requesting Pongo bootr\n");

    /* Make the next shell write wait until the command handler has run. */
    result = libusb_control_transfer(handle, 0x21, 4, 0, 0, NULL, 0,
                                     PONGO_USB_TIMEOUT_MS);
    if (result < 0) {
        fprintf(stderr, "prepare command channel: %s\n",
                libusb_error_name(result));
        goto cleanup;
    }
    result = libusb_control_transfer(handle, 0x21, 3, 0, 0,
                                     (unsigned char *)boot_command,
                                     (uint16_t)(sizeof(boot_command)),
                                     PONGO_USB_TIMEOUT_MS);
    if (result < 0) {
        fprintf(stderr, "bootr request failed or device disconnected: %s\n",
                libusb_error_name(result));
        fprintf(stderr, "Check the phone screen before deciding whether it ran.\n");
        goto cleanup;
    }

    fprintf(stderr, "bootr request accepted. Inspect the phone screen for the payload's marker or report.\n");
    result = 0;

cleanup:
    if (file) {
        fclose(file);
    }
    if (devices) {
        libusb_free_device_list(devices, 1);
    }
    if (handle) {
        if (claimed) {
            libusb_release_interface(handle, 0);
        }
        libusb_close(handle);
    }
    libusb_exit(NULL);
    free(image);
    return result < 0 ? 1 : 0;
}
