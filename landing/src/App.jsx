import './index.css';
import Navbar from './components/Navbar/Navbar';
import Hero from './components/Hero/Hero';
import GpuProfiling from './components/GpuProfiling/GpuProfiling';
import Features from './components/Features/Features';
import AiIntegration from './components/AiIntegration/AiIntegration';
import InstantSetup from './components/InstantSetup/InstantSetup';
import CorePlatform from './components/CorePlatform/CorePlatform';
import Compliance from './components/Compliance/Compliance';
import PlatformFeatures from './components/PlatformFeatures/PlatformFeatures';
import CTASection from './components/CTASection/CTASection';
import Footer from './components/Footer/Footer';

function App() {
  return (
    <div className="app-container">
      <Navbar />
      <Hero />
      <GpuProfiling />
      <Features />
      <AiIntegration />
      <InstantSetup />
      <CorePlatform />
      <Compliance />
      <PlatformFeatures />
      <CTASection />
      <Footer />
    </div>
  );
}

export default App;
