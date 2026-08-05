import { QRCodeSVG } from 'qrcode.react';

interface IdentityQrCodeProps {
  value: string;
  size?: number;
}

export default function IdentityQrCode({ value, size = 240 }: IdentityQrCodeProps) {
  return (
    <div className="identity-qr-code">
      <QRCodeSVG value={value} size={size} level="M" includeMargin />
    </div>
  );
}
