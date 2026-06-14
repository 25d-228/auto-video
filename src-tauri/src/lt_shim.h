#pragma once
// Declarations of the C++ functions the cxx bridge (src/lt.rs) calls.
//
// This header is #included by the cxx-generated header ("auto-video/src/lt.rs.h")
// AFTER the shared structs LtFile / LtStatus are defined, so they are in scope
// here without any include of the generated header (which would be a cycle).
// lt_shim.cpp includes the generated header first, then this is pulled in.
#include "rust/cxx.h"
#include <cstdint>

// The cxx-generated .cc includes this header BEFORE it defines the shared
// structs, so forward-declare them here; full definitions arrive via the
// generated "auto-video/src/lt.rs.h" (included first by lt_shim.cpp).
struct LtFile;
struct LtStatus;

bool lt_init();
rust::String lt_add(rust::Str magnet, rust::Str save_path,
                    rust::Slice<const std::uint32_t> only_files);
rust::Vec<LtFile> lt_list_files(rust::Str magnet, std::uint32_t timeout_ms);
LtStatus lt_status(rust::Str id);
void lt_pause(rust::Str id);
void lt_resume(rust::Str id);
void lt_remove(rust::Str id, bool delete_files);
