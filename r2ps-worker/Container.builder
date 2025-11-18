# Stage 1: Build OpenSC and dependencies
FROM eclipse-temurin:21-jdk-jammy AS opensc-builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    autoconf \
    automake \
    libtool \
    pkg-config \
    libpcsclite-dev \
    libssl-dev \
    libreadline-dev \
    zlib1g-dev \
    wget \
    softhsm2 \
    libsofthsm2 \
    libengine-pkcs11-openssl \
    && rm -rf /var/lib/apt/lists/*

# Download and build OpenSC 0.26.1
WORKDIR /tmp/opensc
RUN wget https://github.com/OpenSC/OpenSC/releases/download/0.26.1/opensc-0.26.1.tar.gz && \
    tar xzf opensc-0.26.1.tar.gz && \
    cd opensc-0.26.1 && \
    ./configure --prefix=/usr/local \
                --sysconfdir=/etc \
                --enable-pcsc \
                --enable-openssl && \
    make -j$(nproc) && \
    make install DESTDIR=/opensc-install && \
    # Create config directory if it doesn't exist
    mkdir -p /opensc-install/etc/opensc


# Copy setup and configuratioin resources
COPY r2ps-worker/softhsm/kek.bin /opt/kek.bin
COPY r2ps-worker/softhsm/softhsm2.conf /opt/softhsm2.conf
COPY r2ps-worker/softhsm/softhsm2-p256.conf /opt/softhsm2-p256.conf
COPY r2ps-worker/softhsm/softhsm2-p384.conf /opt/softhsm2-p384.conf
COPY r2ps-worker/softhsm/softhsm2-p521.conf /opt/softhsm2-p521.conf

# Set environment variables
ENV SOFTHSM2_CONF=/opt/softhsm2.conf
ENV PKCS11LIB=/usr/lib/softhsm/libsofthsm2.so
ENV PKCS11PASSWORD=123456
ENV PKCS11SLOT=0
# Key gen script ENV
ENV PKCS11_MODULE=$PKCS11LIB
ENV LIBPKCS11=/usr/lib/aarch64-linux-gnu/engines-3/libpkcs11.so

RUN ["/usr/bin/softhsm2-util", "--init-token", "--slot", "0", "--label", "'wallet-keys'", \
      "--so-pin", "'1938456231'", "--pin", "'123456'"]

# Import static KEK key to retain keys after image rebuild to match existing key records
RUN pkcs11-tool --module "$PKCS11LIB" \
  --slot-index "$PKCS11SLOT" --login --pin "$PKCS11PASSWORD" \
  --write-object /opt/kek.bin \
  --type secrkey \
  --key-type AES:32 \
  --label kek-1 --id 01 \
  --usage-decrypt --usage-wrap

# Stage 2: Build Java application
FROM eclipse-temurin:21-jdk-jammy AS java-builder

WORKDIR /app

# Copy your application source (uncomment and modify as needed)
# COPY pom.xml .
# COPY src ./src

# Build Java application (uncomment and modify based on your build tool)
# Maven example:
# RUN ./mvnw clean package -DskipTests

# Gradle example:
# RUN ./gradlew build -x test

# For this example, we'll assume the JAR is copied directly
# COPY target/*.jar app.jar

# Stage 3: Runtime with distroless
FROM gcr.io/distroless/java21-debian12:nonroot

# Copy OpenSC libraries and binaries from builder
COPY --from=opensc-builder /opensc-install/usr/local/lib/libopensc.so* /usr/local/lib/
COPY --from=opensc-builder /opensc-install/usr/local/lib/pkcs11/ /usr/local/lib/pkcs11/
COPY --from=opensc-builder /opensc-install/usr/local/bin/pkcs11-tool /usr/local/bin/
COPY --from=opensc-builder /opensc-install/usr/local/bin/opensc-tool /usr/local/bin/
COPY --from=opensc-builder /opensc-install/usr/local/bin/pkcs15-tool /usr/local/bin/

# Copy required shared libraries from builder
COPY --from=opensc-builder /usr/lib/aarch64-linux-gnu/libpcsclite.so.1* /usr/lib/aarch64-linux-gnu/
COPY --from=opensc-builder /usr/lib/aarch64-linux-gnu/libcrypto.so.3* /usr/lib/aarch64-linux-gnu/
COPY --from=opensc-builder /usr/lib/aarch64-linux-gnu/libssl.so.3* /usr/lib/aarch64-linux-gnu/

# Copy Java application
# COPY --from=java-builder /app/app.jar /app/app.jar

# Set library path
ENV LD_LIBRARY_PATH=/usr/local/lib:/usr/lib/aarch64-linux-gnu


# Set working directory
WORKDIR /app



# Run the application
# ENTRYPOINT ["java", "-jar", "/app/app.jar"]

# For testing OpenSC tools, you can use:
# ENTRYPOINT ["/usr/local/bin/pkcs11-tool", "--version"]
