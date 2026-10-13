import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Bug, Ellipsis, FolderOpen, House } from "lucide-react";
import mark from "../assets/kreokit-mark.png";
import { IMAGE_EXTENSIONS } from "../lib/images";
import { openPaths } from "../store/session";
import { Menu, MenuEntry, toast } from "../ui";

/** Shows the open-file dialog (one or many images) and opens what was picked. */
export async function chooseImages() {
  const picked = await open({
    multiple: true,
    directory: false,
    filters: [{ name: "Images", extensions: IMAGE_EXTENSIONS }],
  });
  if (picked) await openPaths(picked);
}

/**
 * The 48 px app bar: logo mark, title, the ⋯ menu and screen actions (`children`).
 * Empty parts are window drag regions; macOS draws the window buttons inside it.
 */
export function TopBar({
  title,
  onHome,
  children,
}: {
  title: React.ReactNode;
  /** Adds "Back to Home" to the menu. */
  onHome?: () => void;
  children?: React.ReactNode;
}) {
  const items: MenuEntry[] = [
    { label: "Open images…", icon: FolderOpen, shortcut: "⌘O", onSelect: () => void chooseImages() },
    ...(onHome ? [{ label: "Back to Home", icon: House, onSelect: onHome }] : []),
    "separator",
    {
      label: "Report a problem",
      icon: Bug,
      onSelect: () => void invoke("report_problem").catch((e) => toast(String(e), "error")),
    },
  ];
  return (
    <header className="topbar" data-tauri-drag-region>
      <img src={mark} alt="" className="topbar-mark" width={24} height={24} data-tauri-drag-region />
      <div className="topbar-title" data-tauri-drag-region>
        {title}
      </div>
      <div className="topbar-actions">
        <Menu buttonLabel="App menu" icon={Ellipsis} variant="ghost" align="end" items={items} />
        {children}
      </div>
    </header>
  );
}
