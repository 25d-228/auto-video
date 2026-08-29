export type FilenameNormalizationCategory = "adult" | "vr";
export type FilenameNormalizationStatus =
  | "already-canonical"
  | "ready"
  | "unresolved"
  | "conflicting"
  | "unsafe";

export type FilenameNormalizationMember = {
  currentRelativeFilename: string;
  proposedRelativeFilename: string | null;
};

export type FilenameNormalizationEntry = {
  id: string;
  status: FilenameNormalizationStatus;
  localCode: string | null;
  provider: "FANZA" | "JavDB" | null;
  providerId: string | null;
  verifiedCode: string | null;
  reason: string;
  members: FilenameNormalizationMember[];
};

export type FilenameNormalizationPlan = {
  id: string;
  category: FilenameNormalizationCategory;
  scanGeneration: string;
  entries: FilenameNormalizationEntry[];
};

const unsignedU64Pattern = /^\d{1,20}$/;
const sha1Pattern = /^[a-f0-9]{40}$/;
const statuses = new Set<FilenameNormalizationStatus>([
  "already-canonical",
  "ready",
  "unresolved",
  "conflicting",
  "unsafe",
]);

function requiredText(value: string | undefined, maximum: number) {
  return value !== undefined && value !== "" && value.length <= maximum ? value : null;
}

export function parseFilenameNormalizationPlan(
  value: unknown,
  expectedCategory: FilenameNormalizationCategory,
  expectedScanGeneration: string,
): FilenameNormalizationPlan {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
    throw new Error("The native filename audit returned invalid data.");
  }
  const fields = value as string[];
  if (
    fields.length < 5 ||
    fields[0] !== "filename-normalization-v1" ||
    !sha1Pattern.test(fields[1]) ||
    fields[2] !== expectedCategory ||
    fields[3] !== expectedScanGeneration ||
    !unsignedU64Pattern.test(fields[3]) ||
    !/^\d{1,6}$/.test(fields[4])
  ) {
    throw new Error("The native filename audit returned invalid data.");
  }
  const entryCount = Number(fields[4]);
  const entries: FilenameNormalizationEntry[] = [];
  const entryIds = new Set<string>();
  let index = 5;
  for (let entryIndex = 0; entryIndex < entryCount; entryIndex += 1) {
    const id = fields[index];
    const status = fields[index + 1] as FilenameNormalizationStatus | undefined;
    const localCode = fields[index + 2];
    const provider = fields[index + 3];
    const providerId = fields[index + 4];
    const verifiedCode = fields[index + 5];
    const reason = requiredText(fields[index + 6], 1_024);
    const memberCountText = fields[index + 7];
    if (
      id === undefined ||
      !sha1Pattern.test(id) ||
      entryIds.has(id) ||
      status === undefined ||
      !statuses.has(status) ||
      reason === null ||
      memberCountText === undefined ||
      !/^\d{1,6}$/.test(memberCountText) ||
      (provider !== "" && provider !== "FANZA" && provider !== "JavDB") ||
      ((provider === "") !== (providerId === "" || providerId === undefined)) ||
      ((provider === "") !== (verifiedCode === "" || verifiedCode === undefined)) ||
      (status === "ready" && provider === "")
    ) {
      throw new Error("The native filename audit returned invalid data.");
    }
    const memberCount = Number(memberCountText);
    index += 8;
    const members: FilenameNormalizationMember[] = [];
    const currentNames = new Set<string>();
    for (let memberIndex = 0; memberIndex < memberCount; memberIndex += 1) {
      const current = requiredText(fields[index], 4_096);
      const proposed = fields[index + 1];
      if (
        current === null ||
        currentNames.has(current) ||
        proposed === undefined ||
        proposed.length > 4_096 ||
        (status === "ready" && proposed === "")
      ) {
        throw new Error("The native filename audit returned invalid data.");
      }
      currentNames.add(current);
      members.push({
        currentRelativeFilename: current,
        proposedRelativeFilename: proposed === "" ? null : proposed,
      });
      index += 2;
    }
    entryIds.add(id);
    entries.push({
      id,
      status,
      localCode: localCode === "" || localCode === undefined ? null : localCode,
      provider: provider === "" || provider === undefined ? null : provider,
      providerId: providerId === "" || providerId === undefined ? null : providerId,
      verifiedCode: verifiedCode === "" || verifiedCode === undefined ? null : verifiedCode,
      reason,
      members,
    });
  }
  if (index !== fields.length || entries.length !== entryCount) {
    throw new Error("The native filename audit returned invalid data.");
  }
  return {
    id: fields[1],
    category: expectedCategory,
    scanGeneration: expectedScanGeneration,
    entries,
  };
}

export async function auditLibraryFilenames(
  category: FilenameNormalizationCategory,
  scanGeneration: string,
) {
  const value = await window.__TAURI__.core.invoke<unknown>("audit_library_filenames", {
    category,
    scanGeneration,
  });
  return parseFilenameNormalizationPlan(value, category, scanGeneration);
}

export async function applyLibraryFilenameNormalization(
  plan: FilenameNormalizationPlan,
  selectedEntryIds: string[],
) {
  if (
    selectedEntryIds.length === 0 ||
    new Set(selectedEntryIds).size !== selectedEntryIds.length ||
    selectedEntryIds.some(
      (id) => !plan.entries.some((entry) => entry.id === id && entry.status === "ready"),
    )
  ) {
    throw new Error("A current ready filename selection is required.");
  }
  return window.__TAURI__.core.invoke<unknown>(
    "apply_library_filename_normalization",
    {
      category: plan.category,
      planId: plan.id,
      selectedEntryIds,
    },
  );
}

export async function dismissLibraryFilenameNormalization() {
  await window.__TAURI__.core.invoke("dismiss_library_filename_normalization");
}

export type FilenameNormalizationRecovery =
  | { status: "none" }
  | { status: "error" }
  | {
      status: "attention";
      category: FilenameNormalizationCategory;
      planId: string;
      paths: { current: string; proposed: string }[];
    };

export async function loadLibraryFilenameNormalizationRecovery(): Promise<FilenameNormalizationRecovery> {
  const value = await window.__TAURI__.core.invoke<unknown>(
    "load_library_filename_normalization_recovery",
  );
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
    throw new Error("The native filename recovery record returned invalid data.");
  }
  const fields = value as string[];
  if (fields.length === 1 && fields[0] === "none") return { status: "none" };
  if (
    fields.length < 4 ||
    fields[0] !== "attention" ||
    (fields[1] !== "adult" && fields[1] !== "vr") ||
    !sha1Pattern.test(fields[2]) ||
    !/^\d{1,6}$/.test(fields[3]) ||
    fields.length !== 4 + Number(fields[3]) * 2
  ) {
    throw new Error("The native filename recovery record returned invalid data.");
  }
  const paths = [];
  for (let index = 4; index < fields.length; index += 2) {
    const current = requiredText(fields[index], 4_096);
    const proposed = requiredText(fields[index + 1], 4_096);
    if (current === null || proposed === null) {
      throw new Error("The native filename recovery record returned invalid data.");
    }
    paths.push({ current, proposed });
  }
  return {
    status: "attention",
    category: fields[1],
    planId: fields[2],
    paths,
  };
}
