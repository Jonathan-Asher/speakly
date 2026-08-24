import { AlignmentType, Document, Packer, Paragraph, TextRun } from "docx";

export interface ExportableSegment {
  startMs: number;
  endMs: number;
  speaker?: string | null;
  text: string;
}

const HEBREW = /[֐-׿]/;

function mmss(ms: number) {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * Word document: one paragraph per segment (speaker-prefixed when labeled),
 * per-paragraph RTL chosen from the first strong character so Hebrew and
 * English segments each lay out natively.
 */
export async function buildDocx(
  segments: ExportableSegment[],
  { title, timestamps }: { title: string; timestamps: boolean },
): Promise<Uint8Array> {
  const children: Paragraph[] = [
    new Paragraph({
      children: [new TextRun({ text: title, bold: true, size: 32 })],
      spacing: { after: 240 },
    }),
  ];

  for (const seg of segments) {
    const rtl = HEBREW.test(seg.text);
    const runs: TextRun[] = [];
    if (timestamps) {
      runs.push(
        new TextRun({ text: `[${mmss(seg.startMs)}] `, color: "888888", size: 18 }),
      );
    }
    if (seg.speaker) {
      runs.push(
        new TextRun({ text: `${seg.speaker}: `, bold: true, rightToLeft: rtl }),
      );
    }
    runs.push(new TextRun({ text: seg.text, rightToLeft: rtl }));
    children.push(
      new Paragraph({
        children: runs,
        bidirectional: rtl,
        alignment: rtl ? AlignmentType.RIGHT : AlignmentType.LEFT,
        spacing: { after: 120 },
      }),
    );
  }

  const doc = new Document({ sections: [{ children }] });
  const blob = await Packer.toBlob(doc);
  return new Uint8Array(await blob.arrayBuffer());
}
