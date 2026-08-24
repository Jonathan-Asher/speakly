import { jsPDF } from "jspdf";
// 66 KB OFL font, inlined at build time; covers Hebrew + basic Latin + digits.
import hebrewFontDataUrl from "../../assets/fonts/NotoSansHebrew-Regular.ttf?inline";
import type { ExportableSegment } from "./docx";

const HEBREW = /[֐-׿]/;
const FONT = "NotoSansHebrew";

let fontRegistered = false;

function ensureFont(doc: jsPDF) {
  const base64 = hebrewFontDataUrl.split(",")[1];
  if (!base64) return;
  // VFS is per-document in current jsPDF, so register on every doc; the
  // module-level flag only guards the base64 split cost. Cheap either way.
  doc.addFileToVFS("NotoSansHebrew-Regular.ttf", base64);
  doc.addFont("NotoSansHebrew-Regular.ttf", FONT, "normal");
  fontRegistered = true;
}

function mmss(ms: number) {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * PDF transcript: timestamp column + text. Direction is chosen per line —
 * lines containing Hebrew render right-aligned RTL in the embedded font;
 * pure-Latin lines render LTR. A single line mixing directions follows the
 * Hebrew ordering (jsPDF has no bidi engine); the docx export is the
 * higher-fidelity rich format for heavily mixed documents.
 */
export function buildPdf(
  segments: ExportableSegment[],
  { title, timestamps }: { title: string; timestamps: boolean },
): Uint8Array {
  const doc = new jsPDF({ unit: "pt", format: "a4" });
  ensureFont(doc);
  void fontRegistered;

  const margin = 48;
  const pageWidth = doc.internal.pageSize.getWidth();
  const pageHeight = doc.internal.pageSize.getHeight();
  const textLeft = margin + (timestamps ? 44 : 0);
  const textWidth = pageWidth - textLeft - margin;
  let y = margin;

  doc.setFont(FONT, "normal");
  doc.setFontSize(16);
  const titleRtl = HEBREW.test(title);
  doc.setR2L(titleRtl);
  doc.text(title, titleRtl ? pageWidth - margin : margin, y, {
    align: titleRtl ? "right" : "left",
  });
  y += 28;

  doc.setFontSize(11);
  for (const seg of segments) {
    const rtl = HEBREW.test(seg.text);
    const label = seg.speaker ? `${seg.speaker}: ${seg.text}` : seg.text;
    doc.setFont(rtl ? FONT : "helvetica", "normal");
    doc.setR2L(rtl);
    const lines: string[] = doc.splitTextToSize(label, textWidth);
    const blockHeight = lines.length * 15 + 6;
    if (y + blockHeight > pageHeight - margin) {
      doc.addPage();
      y = margin;
    }
    if (timestamps) {
      doc.setFont("helvetica", "normal");
      doc.setR2L(false);
      doc.setTextColor(140);
      doc.text(mmss(seg.startMs), margin, y);
      doc.setTextColor(0);
      doc.setFont(rtl ? FONT : "helvetica", "normal");
      doc.setR2L(rtl);
    }
    doc.text(lines, rtl ? pageWidth - margin : textLeft, y, {
      align: rtl ? "right" : "left",
    });
    y += blockHeight;
  }

  doc.setR2L(false);
  return new Uint8Array(doc.output("arraybuffer"));
}
