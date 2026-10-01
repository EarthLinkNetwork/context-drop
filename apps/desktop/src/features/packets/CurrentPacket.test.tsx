import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { CurrentDraft } from "../../lib/api";
import { CurrentPacket } from "./CurrentPacket";

function renderPacket(draft: CurrentDraft | null, overrides = {}) {
  const props = {
    draft,
    busy: false,
    onClear: vi.fn(),
    onOpenItem: vi.fn(),
    onDeleteItem: vi.fn(),
    ...overrides,
  };
  render(<CurrentPacket {...props} />);
  return props;
}

describe("CurrentPacket", () => {
  it("shows an empty state when there is no draft", () => {
    renderPacket(null);
    expect(screen.getByText(/No items captured yet/i)).toBeInTheDocument();
    expect(screen.queryByTestId("item-count")).not.toBeInTheDocument();
  });

  it("renders the item count and recent items list", () => {
    renderPacket({
      packetId: "p1",
      itemCount: 3,
      recentItems: [
        { id: "i1", kind: "image", mimeType: "image/png", byteSize: 2048 },
        { id: "i2", kind: "json", mimeType: "application/json", byteSize: 512 },
        { id: "i3", kind: "text", mimeType: "text/plain", byteSize: 12 },
      ],
    });
    expect(screen.getByTestId("item-count")).toHaveTextContent("3 items");
    expect(screen.getByText("Image")).toBeInTheDocument();
    expect(screen.getByText("JSON")).toBeInTheDocument();
    expect(screen.getByText("Text")).toBeInTheDocument();
    expect(screen.getByText("2.0 KB")).toBeInTheDocument();
    expect(screen.getByText("12 B")).toBeInTheDocument();
  });

  it("shows a text preview and an image thumbnail when provided", () => {
    renderPacket({
      packetId: "p1",
      itemCount: 2,
      recentItems: [
        {
          id: "i1",
          kind: "text",
          mimeType: "text/plain",
          byteSize: 42,
          preview: "TypeError: cannot read properties of undefined",
        },
        {
          id: "i2",
          kind: "image",
          mimeType: "image/png",
          byteSize: 4096,
          imageThumb: "data:image/png;base64,iVBORimg==",
        },
      ],
    });
    expect(screen.getByText(/TypeError: cannot read properties/)).toBeInTheDocument();
    const img = document.querySelector("img.item-thumb") as HTMLImageElement | null;
    expect(img).not.toBeNull();
    expect(img?.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
  });

  it("opens the viewer and deletes an item via its controls", async () => {
    const { onOpenItem, onDeleteItem } = renderPacket({
      packetId: "p1",
      itemCount: 1,
      recentItems: [{ id: "i1", kind: "text", mimeType: "text/plain", byteSize: 12 }],
    });
    // Clicking the row (its kind badge) opens the full-content viewer.
    await userEvent.click(screen.getByText("Text"));
    expect(onOpenItem).toHaveBeenCalledWith(
      expect.objectContaining({ id: "i1" }),
    );
    // The trash control removes that item.
    await userEvent.click(screen.getByRole("button", { name: "Remove item" }));
    expect(onDeleteItem).toHaveBeenCalledWith("i1");
  });

  it("calls onClear when Clear Packet is clicked", async () => {
    const { onClear } = renderPacket({
      packetId: "p1",
      itemCount: 1,
      recentItems: [{ id: "i1", kind: "text", mimeType: "text/plain", byteSize: 1 }],
    });
    await userEvent.click(screen.getByRole("button", { name: "Clear Packet" }));
    expect(onClear).toHaveBeenCalledOnce();
  });
});
