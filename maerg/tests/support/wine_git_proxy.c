#include <windows.h>
#include <stdio.h>
#include <wchar.h>

#define COMMAND_CAPACITY 32768
#define PATH_CAPACITY 32768

/*
 * Wine cannot synchronously wait on a host ELF process. This proxy launches
 * the host-side helper and waits for its atomic completion marker instead.
 */

static int append_quoted(wchar_t *command, size_t *used, const wchar_t *argument) {
    size_t required = wcslen(argument) * 2 + 4;
    if (*used + required >= COMMAND_CAPACITY) {
        return 0;
    }

    command[(*used)++] = L' ';
    command[(*used)++] = L'"';
    size_t backslashes = 0;
    for (const wchar_t *cursor = argument; *cursor != L'\0'; cursor++) {
        if (*cursor == L'\\') {
            backslashes++;
            continue;
        }
        if (*cursor == L'"') {
            while (backslashes > 0) {
                command[(*used)++] = L'\\';
                command[(*used)++] = L'\\';
                backslashes--;
            }
            command[(*used)++] = L'\\';
            command[(*used)++] = L'"';
            continue;
        }
        while (backslashes > 0) {
            command[(*used)++] = L'\\';
            backslashes--;
        }
        command[(*used)++] = *cursor;
    }
    while (backslashes > 0) {
        command[(*used)++] = L'\\';
        command[(*used)++] = L'\\';
        backslashes--;
    }
    command[(*used)++] = L'"';
    command[*used] = L'\0';
    return 1;
}

static int format_succeeded(int written, size_t capacity) {
    return written >= 0 && (size_t)written < capacity;
}

static int windows_to_host_path(
    const wchar_t *windows_path,
    wchar_t *host_path,
    size_t capacity
) {
    wchar_t normalized[PATH_CAPACITY];
    if (wcslen(windows_path) >= PATH_CAPACITY) {
        return 0;
    }
    wcscpy(normalized, windows_path);
    for (wchar_t *cursor = normalized; *cursor != L'\0'; cursor++) {
        if (*cursor == L'\\') {
            *cursor = L'/';
        }
    }

    if ((normalized[0] == L'C' || normalized[0] == L'c')
        && normalized[1] == L':' && normalized[2] == L'/') {
        wchar_t prefix[PATH_CAPACITY];
        DWORD prefix_length = GetEnvironmentVariableW(
            L"WINEPREFIX", prefix, PATH_CAPACITY
        );
        if (prefix_length == 0 || prefix_length >= PATH_CAPACITY) {
            return 0;
        }
        return format_succeeded(
            _snwprintf(
                host_path,
                capacity,
                L"%ls/drive_c/%ls",
                prefix,
                normalized + 3
            ),
            capacity
        );
    }
    if ((normalized[0] == L'Z' || normalized[0] == L'z')
        && normalized[1] == L':' && normalized[2] == L'/') {
        return format_succeeded(
            _snwprintf(host_path, capacity, L"/%ls", normalized + 3),
            capacity
        );
    }
    return 0;
}

int wmain(int argc, wchar_t **argv) {
    wchar_t current_directory[PATH_CAPACITY];
    DWORD current_length = GetCurrentDirectoryW(
        PATH_CAPACITY, current_directory
    );
    if (current_length == 0 || current_length >= PATH_CAPACITY) {
        return 1;
    }

    wchar_t host_directory[PATH_CAPACITY];
    if (!windows_to_host_path(
            current_directory, host_directory, PATH_CAPACITY)) {
        return 1;
    }

    DWORD process_id = GetCurrentProcessId();
    wchar_t marker_windows[PATH_CAPACITY];
    wchar_t marker_host[PATH_CAPACITY];
    int marker_length = _snwprintf(
        marker_windows,
        PATH_CAPACITY,
        L"C:\\windows\\temp\\kvist-git-%lu.status",
        process_id
    );
    if (!format_succeeded(marker_length, PATH_CAPACITY)
        || !windows_to_host_path(
            marker_windows, marker_host, PATH_CAPACITY)) {
        return 1;
    }
    DeleteFileW(marker_windows);

    wchar_t command[COMMAND_CAPACITY] = L"Z:\\usr\\bin\\bash";
    size_t used = wcslen(command);
    if (!append_quoted(
            command, &used, L"/opt/target/wine/git-proxy.sh")
        || !append_quoted(command, &used, marker_host)
        || !append_quoted(command, &used, host_directory)) {
        return 1;
    }
    for (int index = 1; index < argc; index++) {
        wchar_t host_argument[PATH_CAPACITY];
        const wchar_t *argument = argv[index];
        if (windows_to_host_path(
                argument, host_argument, PATH_CAPACITY)) {
            argument = host_argument;
        }
        if (!append_quoted(command, &used, argument)) {
            return 1;
        }
    }

    STARTUPINFOW startup = {0};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process = {0};
    if (!CreateProcessW(
            NULL, command, NULL, NULL, TRUE, 0, NULL, NULL,
            &startup, &process)) {
        return 1;
    }
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);

    for (DWORD waited = 0; waited < 300000; waited += 10) {
        FILE *marker = _wfopen(marker_windows, L"r");
        if (marker != NULL) {
            int status = 1;
            if (fwscanf(marker, L"%d", &status) != 1) {
                status = 1;
            }
            fclose(marker);
            DeleteFileW(marker_windows);
            return status;
        }
        Sleep(10);
    }
    return 1;
}
