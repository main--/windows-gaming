DRIVER := windows-gaming-driver/target/release/windows-gaming-driver
GA_EXE := guest-agent/VfioService.exe
GA_ISO := guest-agent/windows-gaming-ga.iso
OVMF   := ovmf-x64/OVMF_CODE-pure-efi.fd ovmf-x64/OVMF_VARS-pure-efi.fd

all: $(DRIVER) $(GA_ISO) $(OVMF)

test:
	cd windows-gaming-driver && cargo test --release --locked

$(DRIVER):
	cd windows-gaming-driver && cargo build --release --locked

$(GA_EXE): guest-agent/VfioService/VfioService.sln $(wildcard guest-agent/VfioService/VfioService/*.*) $(wildcard guest-agent/VfioService/VfioService/Properties/*)
	cd guest-agent && xbuild /p:Configuration=Release VfioService/VfioService.sln
	cp --preserve=timestamps guest-agent/VfioService/VfioService/bin/Release/VfioService.exe guest-agent

$(GA_ISO): $(GA_EXE) guest-agent/install.bat guest-agent/uninstall.bat
	cd guest-agent && mkisofs -m VfioService -o windows-gaming-ga.iso -r -J -input-charset iso8859-1 -V "windows-gaming-ga" .

ovmf.rpm:
	curl -o ovmf.rpm "https://www.kraxel.org/repos/jenkins/edk2/$(shell curl -s 'https://www.kraxel.org/repos/jenkins/edk2/' | grep -Eo 'edk2.git-ovmf-x64-[-\.a-z0-9]+\.noarch\.rpm' | head -n1)"

ovmf-x64/OVMF_CODE-pure-efi%fd ovmf-x64/OVMF_VARS-pure-efi%fd: ovmf%rpm
	rpm2cpio ovmf.rpm | bsdtar -xvmf - --strip-components 4 './usr/share/edk2.git/ovmf-x64/OVMF_CODE-pure-efi.fd' './usr/share/edk2.git/ovmf-x64/OVMF_VARS-pure-efi.fd'


clean:
	$(RM) ovmf.rpm $(OVMF)
	$(RM) $(GA_EXE) $(GA_ISO)
	cd guest-agent && xbuild /p:Configuration=Release /t:clean VfioService/VfioService.sln
	cd windows-gaming-driver && cargo clean --release



install: all
	install -D $(DRIVER) $(DESTDIR)/usr/bin/windows-gaming-driver
	install -D -m644 ovmf-x64/OVMF_CODE-pure-efi.fd $(DESTDIR)/usr/lib/windows-gaming/ovmf-code.fd
	install -D -m644 ovmf-x64/OVMF_VARS-pure-efi.fd $(DESTDIR)/usr/lib/windows-gaming/ovmf-vars.fd
	install -D -m644 $(GA_ISO) $(DESTDIR)/usr/lib/windows-gaming/windows-gaming-ga.iso
	install -D -m644 ../virtio-win_amd64.vfd $(DESTDIR)/usr/lib/windows-gaming/virtio-win.vfd #FIXME
	install -D -m644 misc/windows.service $(DESTDIR)/lib/systemd/system/windows.service
	install -D -m644 misc/windows.service $(DESTDIR)/lib/systemd/user/windows.service
	install -D -m644 misc/80-vfio.rules $(DESTDIR)/lib/udev/rules.d/80-vfio.rules



.PHONY: OVMF clean all install $(DRIVER) # Simply always run cargo instead of tracking all the rs sources in here
