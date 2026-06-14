// libtorrent-rasterbar shim behind the cxx bridge (src/lt.rs).
//
// Owns one global libtorrent session + a map of info-hash -> torrent. The whole
// point of this backend: deselected files get file_priority = 0 (dont_download),
// so with sparse storage libtorrent NEVER creates them on disk (no 0-byte stubs,
// unlike librqbit's only_files).

// The cxx-generated header defines LtFile / LtStatus and then #includes
// lt_shim.h. Include path prefix is the Cargo package name ("auto-video"); if a
// build complains, it's this line that tracks the crate name.
#include "auto-video/src/lt.rs.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/alert.hpp>
#include <libtorrent/alert_types.hpp>
#include <libtorrent/download_priority.hpp>
#include <libtorrent/error_code.hpp>
#include <libtorrent/file_storage.hpp>
#include <libtorrent/info_hash.hpp>
#include <libtorrent/magnet_uri.hpp>
#include <libtorrent/session.hpp>
#include <libtorrent/settings_pack.hpp>
#include <libtorrent/torrent_flags.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/torrent_info.hpp>
#include <libtorrent/torrent_status.hpp>

namespace {
namespace lt = libtorrent;

struct Entry {
  lt::torrent_handle handle;
  std::vector<std::uint32_t> only_files; // empty = download all
  bool applied = false;                  // selection + resume applied post-metadata
  bool user_paused = false;              // explicit user pause (vs internal add-paused)
};

std::unique_ptr<lt::session> g_ses;
std::mutex g_mu;
std::map<std::string, Entry> g_torrents; // info-hash hex -> entry

template <class H>
std::string to_hex(H const& h) {
  static const char* digits = "0123456789abcdef";
  std::string s;
  s.reserve(static_cast<std::size_t>(h.size()) * 2);
  for (int i = 0; i < int(h.size()); ++i) {
    unsigned char b = static_cast<unsigned char>(h.data()[i]);
    s.push_back(digits[b >> 4]);
    s.push_back(digits[b & 0xf]);
  }
  return s;
}

std::string id_of(lt::info_hash_t const& ih) {
  return ih.has_v1() ? to_hex(ih.v1) : to_hex(ih.v2);
}

// Apply file selection (priority 0 for deselected). Called the instant metadata
// arrives (metadata_received_alert) — before any payload piece is written — so
// deselected files are never created on disk. Caller holds g_mu.
void apply_selection(Entry& e) {
  if (e.applied) return;
  auto ti = e.handle.torrent_file();
  if (!ti) return;
  int n = ti->num_files();
  if (!e.only_files.empty()) {
    std::vector<lt::download_priority_t> prios(
        static_cast<std::size_t>(n), lt::dont_download);
    for (std::uint32_t idx : e.only_files) {
      if (int(idx) < n) prios[idx] = lt::default_priority;
    }
    e.handle.prioritize_files(prios);
  }
  e.applied = true;
}

} // namespace

bool lt_init() {
  std::lock_guard<std::mutex> lk(g_mu);
  if (g_ses) return true;
  lt::settings_pack sp;
  // status_notification carries metadata_received_alert, which we use to apply
  // the file selection the moment metadata arrives.
  sp.set_int(lt::settings_pack::alert_mask, lt::alert_category::status);
  sp.set_str(lt::settings_pack::listen_interfaces, "0.0.0.0:6881");
  try {
    g_ses = std::make_unique<lt::session>(sp);
  } catch (...) {
    return false;
  }
  // Apply file selection the instant metadata arrives. wait_for_alert returns as
  // soon as the alert is posted, so prioritize_files lands within milliseconds —
  // before any payload piece round-trip completes — keeping deselected files off
  // disk. (paused/upload_mode/stop_when_ready all block or pre-empt the metadata
  // fetch on magnets in this build, so the torrent must run normally and be
  // constrained right after metadata instead.)
  {
    lt::session* ses = g_ses.get();
    std::thread([ses] {
      for (;;) {
        ses->wait_for_alert(std::chrono::seconds(1));
        std::vector<lt::alert*> alerts;
        ses->pop_alerts(&alerts);
        for (auto* a : alerts) {
          auto* m = lt::alert_cast<lt::metadata_received_alert>(a);
          if (!m) continue;
          std::lock_guard<std::mutex> lk(g_mu);
          auto it = g_torrents.find(id_of(m->handle.info_hashes()));
          if (it != g_torrents.end()) apply_selection(it->second);
        }
      }
    }).detach();
  }
  return true;
}

rust::String lt_add(rust::Str magnet, rust::Str save_path,
                    rust::Slice<const std::uint32_t> only_files) {
  if (!lt_init()) return rust::String("");
  lt::error_code ec;
  lt::add_torrent_params atp = lt::parse_magnet_uri(std::string(magnet), ec);
  if (ec) return rust::String("");
  atp.save_path = std::string(save_path);
  // parse_magnet_uri defaults the flags to include `paused`; clear it so the
  // torrent actually runs and fetches metadata (paused = no peer/DHT activity =
  // dead-lock at 0%). No upload_mode/stop_when_ready: both block or pre-empt the
  // metadata fetch on magnets in this build. The file selection is applied the
  // instant metadata arrives (see the alert thread in lt_init), before any
  // payload is written. auto_managed off so we control start/stop.
  atp.flags &= ~lt::torrent_flags::paused;
  atp.flags &= ~lt::torrent_flags::auto_managed;
  std::string id = id_of(atp.info_hashes);

  std::lock_guard<std::mutex> lk(g_mu);
  auto it = g_torrents.find(id);
  if (it != g_torrents.end()) {
    // Already managed (e.g. resume re-add): refresh selection intent, re-apply.
    it->second.only_files.assign(only_files.begin(), only_files.end());
    it->second.applied = false;
    return rust::String(id);
  }
  lt::torrent_handle th = g_ses->add_torrent(std::move(atp), ec);
  if (ec || !th.is_valid()) return rust::String("");
  Entry e;
  e.handle = th;
  e.only_files.assign(only_files.begin(), only_files.end());
  g_torrents.emplace(id, std::move(e));
  return rust::String(id);
}

rust::Vec<LtFile> lt_list_files(rust::Str magnet, std::uint32_t timeout_ms) {
  rust::Vec<LtFile> out;
  if (!lt_init()) return out;
  lt::error_code ec;
  lt::add_torrent_params atp = lt::parse_magnet_uri(std::string(magnet), ec);
  if (ec) return out;
  std::string id = id_of(atp.info_hashes);

  // Reuse the torrent if it's already managed (downloading); otherwise add a
  // temporary metadata-only torrent (upload_mode writes no data).
  lt::torrent_handle th;
  bool temporary = false;
  {
    std::lock_guard<std::mutex> lk(g_mu);
    auto it = g_torrents.find(id);
    if (it != g_torrents.end()) th = it->second.handle;
  }
  if (!th.is_valid()) {
    auto tmp = std::filesystem::temp_directory_path() / "av-lt-meta";
    std::error_code fec;
    std::filesystem::create_directories(tmp, fec);
    atp.save_path = tmp.string();
    // Run normally to fetch metadata; no payload is written during the
    // metadata phase, and we remove the torrent (delete_files) the instant
    // metadata lands. (upload_mode/stop_when_ready block the metadata fetch.)
    atp.flags &= ~lt::torrent_flags::paused; // must run to fetch metadata
    th = g_ses->add_torrent(std::move(atp), ec);
    if (ec || !th.is_valid()) return out;
    temporary = true;
  }

  auto deadline =
      std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
  std::shared_ptr<const lt::torrent_info> ti;
  for (;;) {
    ti = th.torrent_file();
    if (ti && ti->num_files() > 0) break;
    if (std::chrono::steady_clock::now() >= deadline) break;
    std::this_thread::sleep_for(std::chrono::milliseconds(200));
  }

  if (ti && ti->num_files() > 0) {
    lt::file_storage const& fs = ti->files();
    for (int i = 0; i < fs.num_files(); ++i) {
      LtFile f;
      f.index = static_cast<std::uint32_t>(i);
      f.path = rust::String(fs.file_path(lt::file_index_t(i)));
      f.size = static_cast<std::uint64_t>(fs.file_size(lt::file_index_t(i)));
      out.push_back(std::move(f));
    }
  }

  if (temporary && g_ses) {
    g_ses->remove_torrent(th, lt::session::delete_files);
  }
  return out;
}

LtStatus lt_status(rust::Str id) {
  LtStatus s;
  s.valid = false;
  s.has_metadata = false;
  s.progress = 0.0;
  s.download_rate = 0;
  s.finished = false;
  std::lock_guard<std::mutex> lk(g_mu);
  auto it = g_torrents.find(std::string(id));
  if (it == g_torrents.end()) return s;
  Entry& e = it->second;
  if (!e.handle.is_valid()) return s;
  lt::torrent_status ts = e.handle.status();
  s.valid = true;
  s.has_metadata = ts.has_metadata;

  // Lazily apply file selection + resume the first time metadata is available.
  if (ts.has_metadata && !e.applied) {
    apply_selection(e);
    ts = e.handle.status(); // refresh after resume
  }

  s.progress = static_cast<double>(ts.progress);
  s.download_rate =
      static_cast<std::uint64_t>(std::max(0, ts.download_payload_rate));
  s.finished = ts.is_finished;
  if (ts.errc) {
    s.state = rust::String("error");
    s.error = rust::String(ts.errc.message());
  } else if (e.user_paused) {
    s.state = rust::String("paused");
  } else if (ts.is_finished) {
    s.state = rust::String("done");
  } else {
    s.state = rust::String("downloading");
  }
  return s;
}

void lt_pause(rust::Str id) {
  std::lock_guard<std::mutex> lk(g_mu);
  auto it = g_torrents.find(std::string(id));
  if (it == g_torrents.end()) return;
  it->second.user_paused = true;
  if (it->second.handle.is_valid()) it->second.handle.pause();
}

void lt_resume(rust::Str id) {
  std::lock_guard<std::mutex> lk(g_mu);
  auto it = g_torrents.find(std::string(id));
  if (it == g_torrents.end()) return;
  it->second.user_paused = false;
  if (it->second.handle.is_valid()) it->second.handle.resume();
}

void lt_remove(rust::Str id, bool delete_files) {
  std::lock_guard<std::mutex> lk(g_mu);
  auto it = g_torrents.find(std::string(id));
  if (it == g_torrents.end()) return;
  if (g_ses && it->second.handle.is_valid()) {
    g_ses->remove_torrent(it->second.handle,
                          delete_files ? lt::session::delete_files
                                       : lt::remove_flags_t{});
  }
  g_torrents.erase(it);
}
