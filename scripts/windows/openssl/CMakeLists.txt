cmake_minimum_required(VERSION 3.24)
project(cipherclipboard_fetch_openssl LANGUAGES NONE)

include(FetchContent)

# Portable Windows OpenSSL distribution (headers + libcrypto.lib + libcrypto-*.dll).
# Override -DOPENSSL_ZIP_URL=... to use a specific version or internal mirror.
set(OPENSSL_ZIP_URL
    "https://download.firedaemon.com/FireDaemon-OpenSSL/openssl-3.5.9.zip"
    CACHE STRING "URL to a portable Windows OpenSSL ZIP archive"
)

# Architecture subdirectory inside multi-arch FireDaemon ZIPs (x64, x86, arm64).
if(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "ARM64|aarch64")
    set(_default_arch "arm64")
else()
    set(_default_arch "x64")
endif()
set(OPENSSL_ARCH "${_default_arch}" CACHE STRING "Target architecture subfolder (x64, x86, arm64)")

FetchContent_Declare(
    openssl_prebuilt
    URL "${OPENSSL_ZIP_URL}"
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(openssl_prebuilt)

# Support both multi-arch ZIPs (<root>/x64/include) and single-arch ZIPs (<root>/include).
if(EXISTS "${openssl_prebuilt_SOURCE_DIR}/${OPENSSL_ARCH}/include")
    set(RESOLVED_OPENSSL_DIR "${openssl_prebuilt_SOURCE_DIR}/${OPENSSL_ARCH}")
else()
    set(RESOLVED_OPENSSL_DIR "${openssl_prebuilt_SOURCE_DIR}")
endif()

if(NOT EXISTS "${RESOLVED_OPENSSL_DIR}/include/openssl/evp.h")
    message(FATAL_ERROR
        "Expected OpenSSL headers not found under '${RESOLVED_OPENSSL_DIR}/include/openssl'. "
        "Check OPENSSL_ZIP_URL and OPENSSL_ARCH."
    )
endif()

file(TO_NATIVE_PATH "${RESOLVED_OPENSSL_DIR}" NATIVE_OPENSSL_DIR)
file(TO_NATIVE_PATH "${RESOLVED_OPENSSL_DIR}/bin" NATIVE_OPENSSL_BIN_DIR)

message(STATUS "OpenSSL (libcrypto) ready at: ${NATIVE_OPENSSL_DIR}")
message(STATUS "Run in PowerShell before `cargo build` / `cargo run`:")
message(STATUS "  $env:OPENSSL_DIR = \"${NATIVE_OPENSSL_DIR}\"")
message(STATUS "  $env:PATH = \"${NATIVE_OPENSSL_BIN_DIR};$env:PATH\"")
