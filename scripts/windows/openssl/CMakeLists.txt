cmake_minimum_required(VERSION 3.24)
project(cipherclipboard_fetch_openssl LANGUAGES NONE)

# Mode selection:
#   - prebuilt (default): downloads a portable Windows OpenSSL ZIP via FetchContent (no Perl/NASM)
#   - source:             builds only libcrypto from official OpenSSL sources via ExternalProject_Add
set(OPENSSL_MODE "prebuilt" CACHE STRING "OpenSSL acquisition mode: prebuilt | source")
set_property(CACHE OPENSSL_MODE PROPERTY STRINGS prebuilt source)

if(OPENSSL_MODE STREQUAL "source")
    enable_language(C)
    include(ExternalProject)

    find_program(PERL_EXECUTABLE perl)
    if(NOT PERL_EXECUTABLE)
        message(FATAL_ERROR
            "Perl is required to build OpenSSL from source on Windows "
            "(e.g. `winget install StrawberryPerl.StrawberryPerl`). "
            "Or use -DOPENSSL_MODE=prebuilt."
        )
    endif()

    set(OPENSSL_SOURCE_URL
        "https://github.com/openssl/openssl/releases/download/openssl-3.5.0/openssl-3.5.0.tar.gz"
        CACHE STRING "URL to official OpenSSL source tarball"
    )
    option(OPENSSL_SHARED "Build shared libcrypto DLL + import lib (matches libsqlite3-sys dylib=libcrypto)" ON)

    if(CMAKE_SIZEOF_VOID_P EQUAL 8)
        if(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "ARM64|aarch64")
            set(_openssl_target "VC-WIN64-ARM")
        else()
            set(_openssl_target "VC-WIN64A")
        endif()
    else()
        set(_openssl_target "VC-WIN32")
    endif()

    set(RESOLVED_OPENSSL_DIR "${CMAKE_BINARY_DIR}/openssl-install")
    file(MAKE_DIRECTORY "${RESOLVED_OPENSSL_DIR}")
    file(TO_NATIVE_PATH "${RESOLVED_OPENSSL_DIR}" NATIVE_OPENSSL_DIR)
    file(TO_NATIVE_PATH "${RESOLVED_OPENSSL_DIR}/ssl" NATIVE_OPENSSL_SSL_DIR)
    file(TO_NATIVE_PATH "${RESOLVED_OPENSSL_DIR}/bin" NATIVE_OPENSSL_BIN_DIR)

    set(_configure_args
        "${_openssl_target}"
        "--prefix=${NATIVE_OPENSSL_DIR}"
        "--openssldir=${NATIVE_OPENSSL_SSL_DIR}"
        no-apps
        no-docs
        no-tests
        no-ssl3
    )

    if(NOT OPENSSL_SHARED)
        list(APPEND _configure_args no-shared)
    endif()

    find_program(NASM_EXECUTABLE nasm)
    if(NOT NASM_EXECUTABLE)
        message(STATUS "NASM not found; configuring OpenSSL with no-asm")
        list(APPEND _configure_args no-asm)
    endif()

    ExternalProject_Add(
        openssl_libcrypto
        URL "${OPENSSL_SOURCE_URL}"
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
        BUILD_IN_SOURCE 1
        CONFIGURE_COMMAND "${PERL_EXECUTABLE}" Configure ${_configure_args}
        BUILD_COMMAND nmake build_libs
        INSTALL_COMMAND nmake install_sw
    )

    message(STATUS "Configured OpenSSL (source -> libcrypto only).")
    message(STATUS "Run from an x64 Native Tools Command Prompt for VS:")
    message(STATUS "  cmake --build \"${CMAKE_BINARY_DIR}\"")
    message(STATUS "Then in PowerShell before `cargo build` / `cargo run`:")
    message(STATUS "  $env:OPENSSL_DIR = \"${NATIVE_OPENSSL_DIR}\"")
    if(OPENSSL_SHARED)
        message(STATUS "  $env:PATH = \"${NATIVE_OPENSSL_BIN_DIR};$env:PATH\"")
    endif()
else()
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
endif()
