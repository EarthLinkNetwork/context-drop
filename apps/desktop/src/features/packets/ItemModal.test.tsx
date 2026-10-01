import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ItemModal } from "./ItemModal";

const mocks = vi.hoisted(() => ({
  itemFullText: vi.fn(),
  itemFullImage: vi.fn(),
}));
vi.mock("../../lib/api", () => ({ api: mocks }));

describe("ItemModal", () => {
  it("loads and shows the full text, then closes", async () => {
    mocks.itemFullText.mockResolvedValue("full log line 1\nfull log line 2\n");
    const onClose = vi.fn();
    render(
      <ItemModal
        packetId="p1"
        item={{ id: "i1", kind: "text", mimeType: "text/plain", byteSize: 100 }}
        onClose={onClose}
      />,
    );
    expect(await screen.findByText(/full log line 1/)).toBeInTheDocument();
    expect(mocks.itemFullText).toHaveBeenCalledWith("p1", "i1");
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("loads a large image for image items", async () => {
    mocks.itemFullImage.mockResolvedValue("data:image/png;base64,AAAA");
    render(
      <ItemModal
        packetId="p1"
        item={{
          id: "i2",
          kind: "image",
          mimeType: "image/png",
          byteSize: 100,
          imageThumb: "data:image/png;base64,xx",
        }}
        onClose={vi.fn()}
      />,
    );
    await waitFor(() => expect(mocks.itemFullImage).toHaveBeenCalledWith("p1", "i2"));
    const img = document.querySelector("img.modal-image") as HTMLImageElement | null;
    expect(img?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
  });
});
